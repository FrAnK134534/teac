use super::cfg::Cfg;
use super::dominator::DominatorInfo;
use super::FunctionPass;
use crate::common::graph::BackwardLiveness;
use crate::ir::function::{BasicBlock, BlockLabel, Function};
use crate::ir::stmt::{OperandRole, Stmt, StmtInner};
use crate::ir::types::Dtype;
use crate::ir::value::{LocalVariable, Operand};
use std::collections::{HashMap, HashSet, VecDeque};

pub struct Mem2RegPass;

impl FunctionPass for Mem2RegPass {
    fn run(&self, func: &mut Function) {
        let Function {
            blocks: ref mut blocks_opt,
            ref mut next_vreg,
            ..
        } = *func;

        let Some(blocks) = blocks_opt.as_mut() else {
            return;
        };
        if blocks.is_empty() {
            return;
        }

        let cfg = Cfg::from_blocks(blocks);
        let dom_info = DominatorInfo::compute(cfg.graph());
        let analysis = AllocaAnalysis::from_blocks(blocks);
        let promotable = analysis.promotable_vars(&dom_info);

        if !promotable.is_empty() {
            let mut phis = PhiPlacement::new(cfg.num_blocks());
            Self::place_phis(&promotable, &mut phis, &cfg, &dom_info, next_vreg);

            let mut renamer = Renamer::new(
                blocks,
                &cfg,
                &dom_info,
                &mut phis,
                promotable.keys().copied().collect(),
            );
            renamer.run();
            *blocks = renamer.finish();
        }
    }
}

impl Mem2RegPass {
    fn place_phis(
        promotable: &HashMap<usize, VarUsage>,
        phi_placement: &mut PhiPlacement,
        cfg: &Cfg,
        dom_info: &DominatorInfo,
        next_vreg: &mut usize,
    ) {
        for (&var_idx, info) in promotable.iter() {
            if !info.has_load {
                continue;
            }

            let n = cfg.num_blocks();
            let gen: Vec<bool> = (0..n)
                .map(|b| info.load_before_store_blocks.contains(&b))
                .collect();
            let kill: Vec<bool> = (0..n).map(|b| info.def_blocks.contains(&b)).collect();
            let liveness = BackwardLiveness::<bool>::compute(&gen, &kill, cfg.graph());

            let mut worklist: VecDeque<usize> = info.def_blocks.iter().copied().collect();

            while let Some(b) = worklist.pop_front() {
                for &y in dom_info.dominance_frontier(b) {
                    if !liveness.live_in[y] {
                        continue;
                    }
                    if phi_placement.has_phi(y, var_idx) {
                        continue;
                    }

                    let idx = *next_vreg;
                    *next_vreg += 1;
                    let dst = Operand::from(LocalVariable::new(Dtype::I32, idx, None));
                    phi_placement.insert_phi(y, var_idx, dst);

                    if !info.def_blocks.contains(&y) {
                        worklist.push_back(y);
                    }
                }
            }
        }
    }
}

struct AllocaAnalysis {
    usage: HashMap<usize, VarUsage>,
}

impl AllocaAnalysis {
    /// Constructs an `AllocaAnalysis` by scanning all basic blocks.
    ///
    /// First identifies alloca instructions that allocate i32 pointers as
    /// promotion candidates, then analyzes their load/store usage patterns
    /// across all blocks.
    fn from_blocks(blocks: &[BasicBlock]) -> Self {
        let candidates = Self::collect_candidates(blocks);
        let usage = Self::analyze_usage(blocks, &candidates);
        Self { usage }
    }

    /// Returns the subset of analyzed variables that are safe to promote to SSA form.
    ///
    /// A variable is promotable if:
    /// 1. It has at least one store (otherwise there's nothing to promote).
    /// 2. It is not used in any invalid way (e.g., address taken for non-load/store).
    /// 3. Every block that loads before storing is dominated by at least one
    ///    definition block, ensuring reads always see a defined value.
    ///
    /// Single-definition variables are rate-limited to avoid exploding the
    /// register allocator's interference graph on stress tests.
    fn promotable_vars(&self, dom_info: &DominatorInfo) -> HashMap<usize, VarUsage> {
        let mut multi_def = HashMap::new();
        let mut single_def = HashMap::new();

        for (&var, info) in &self.usage {
            if info.invalid || !info.has_store {
                continue;
            }

            let mut ok = true;
            for &block in &info.load_before_store_blocks {
                let has_dom_def = info
                    .def_blocks
                    .iter()
                    .any(|&def_block| def_block != block && dom_info.dominates(def_block, block));
                if !has_dom_def {
                    ok = false;
                    break;
                }
            }

            if ok {
                if info.def_blocks.len() <= 1 {
                    single_def.insert(var, info.clone());
                } else {
                    multi_def.insert(var, info.clone());
                }
            }
        }

        // Promoting single-def variables (defined in exactly one block)
        // extends live ranges: the stored value stays live from its
        // definition until its last use, instead of being killed at
        // the store.  For functions with very many single-def locals
        // (e.g., stress tests with thousands of variables), this causes
        // the O(n²) register allocator's interference graph to explode.
        //
        // Promote single-def variables only when the count is manageable.
        const SINGLE_DEF_LIMIT: usize = 256;
        if single_def.len() <= SINGLE_DEF_LIMIT {
            multi_def.extend(single_def);
        }

        multi_def
    }

    /// Scans all blocks for alloca instructions that produce `*i32` pointers.
    ///
    /// Returns the set of virtual register indices for these allocas. Only i32
    /// pointer allocas are considered because the current implementation only
    /// supports promoting scalar integer values.
    fn collect_candidates(blocks: &[BasicBlock]) -> HashSet<usize> {
        let mut candidates = HashSet::new();
        for stmt in blocks.iter().flat_map(|block| block.stmts.iter()) {
            if let StmtInner::Alloca(a) = &stmt.inner {
                if let Some(idx) = a.dst.vreg_index() {
                    if let Dtype::Pointer { pointee } = a.dst.dtype() {
                        if matches!(pointee.as_ref(), Dtype::I32) {
                            candidates.insert(idx);
                        }
                    }
                }
            }
        }
        candidates
    }

    /// Analyzes how each candidate alloca variable is used across all blocks.
    ///
    /// For each candidate, tracks:
    /// - `def_blocks`: blocks containing stores (definitions).
    /// - `load_before_store_blocks`: blocks that load the variable before any
    ///   store within the same block (upward-exposed uses).
    /// - `has_store` / `has_load`: whether stores/loads exist at all.
    /// - `invalid`: set if the variable is used in a non-promotable way (e.g.,
    ///   passed to a call or used as a general operand rather than load/store).
    fn analyze_usage(
        blocks: &[BasicBlock],
        candidates: &HashSet<usize>,
    ) -> HashMap<usize, VarUsage> {
        let mut usage: HashMap<usize, VarUsage> = candidates
            .iter()
            .map(|&v| (v, VarUsage::default()))
            .collect();

        for (b_idx, block) in blocks.iter().enumerate() {
            let mut store_seen: HashSet<usize> = HashSet::new();

            for stmt in &block.stmts {
                for op_ref in stmt.operands() {
                    let Some(idx) = op_ref.operand.vreg_index() else {
                        continue;
                    };
                    let Some(info) = usage.get_mut(&idx) else {
                        continue;
                    };
                    match op_ref.role {
                        OperandRole::LoadPtr => {
                            if !store_seen.contains(&idx) {
                                info.load_before_store_blocks.insert(b_idx);
                            }
                            info.has_load = true;
                        }
                        OperandRole::StorePtr => {
                            store_seen.insert(idx);
                            info.has_store = true;
                            info.def_blocks.insert(b_idx);
                        }
                        OperandRole::Def => {}
                        OperandRole::Use => {
                            info.invalid = true;
                        }
                    }
                }
            }
        }

        usage
    }
}

#[derive(Clone, Default)]
struct VarUsage {
    def_blocks: HashSet<usize>,
    load_before_store_blocks: HashSet<usize>,
    has_store: bool,
    has_load: bool,
    invalid: bool,
}

#[derive(Clone)]
struct PhiInfo {
    var: usize,
    dst: Operand,
    incomings: Vec<(BlockLabel, Operand)>,
}

struct PhiPlacement {
    nodes: Vec<Vec<PhiInfo>>,
    lookup: Vec<HashMap<usize, usize>>,
}

impl PhiPlacement {
    fn new(num_blocks: usize) -> Self {
        Self {
            nodes: vec![Vec::new(); num_blocks],
            lookup: vec![HashMap::new(); num_blocks],
        }
    }

    fn insert_phi(&mut self, block: usize, var: usize, dst: Operand) {
        let phi = PhiInfo {
            var,
            dst,
            incomings: Vec::new(),
        };
        self.lookup[block].insert(var, self.nodes[block].len());
        self.nodes[block].push(phi);
    }

    fn has_phi(&self, block: usize, var: usize) -> bool {
        self.lookup[block].contains_key(&var)
    }

    fn phis_at(&self, block: usize) -> &[PhiInfo] {
        &self.nodes[block]
    }

    fn phis_at_mut(&mut self, block: usize) -> &mut [PhiInfo] {
        &mut self.nodes[block]
    }
}

struct Renamer<'a> {
    blocks: &'a [BasicBlock],
    cfg: &'a Cfg,
    dom_info: &'a DominatorInfo,
    phi_placement: &'a mut PhiPlacement,
    promoted: HashSet<usize>,
    var_stack: HashMap<usize, Vec<Operand>>,
    alias_map: HashMap<usize, Operand>,
    rewritten: Vec<Vec<Stmt>>,
}

impl<'a> Renamer<'a> {
    fn new(
        blocks: &'a [BasicBlock],
        cfg: &'a Cfg,
        dom_info: &'a DominatorInfo,
        phi_placement: &'a mut PhiPlacement,
        promoted: HashSet<usize>,
    ) -> Self {
        let mut var_stack = HashMap::new();
        for var in promoted.iter().copied() {
            var_stack.insert(var, Vec::new());
        }
        Self {
            blocks,
            cfg,
            dom_info,
            phi_placement,
            promoted,
            var_stack,
            alias_map: HashMap::new(),
            rewritten: vec![Vec::new(); blocks.len()],
        }
    }

    fn run(&mut self) {
        for root in self.dom_info.dom_tree_roots() {
            self.clear_state();
            self.rename_block(root);
        }
    }

    fn finish(self) -> Vec<BasicBlock> {
        let mut out = Vec::with_capacity(self.blocks.len());

        for (i, block) in self.blocks.iter().enumerate() {
            let mut stmts = Vec::new();
            for phi in self.phi_placement.phis_at(i) {
                stmts.push(Stmt::as_phi(phi.dst.clone(), phi.incomings.clone()));
            }
            stmts.extend(self.rewritten[i].iter().cloned());
            out.push(BasicBlock {
                label: block.label.clone(),
                stmts,
            });
        }

        out
    }

    fn rename_block(&mut self, block_idx: usize) {
        let mut pushed_vars: Vec<usize> = Vec::new();
        let mut added_aliases: Vec<usize> = Vec::new();

        for phi in self.phi_placement.phis_at(block_idx) {
            if let Some(stack) = self.var_stack.get_mut(&phi.var) {
                stack.push(phi.dst.clone());
                pushed_vars.push(phi.var);
            }
        }

        for stmt in &self.blocks[block_idx].stmts {
            match &stmt.inner {
                StmtInner::Alloca(a) => {
                    if let Some(idx) = a.dst.vreg_index() {
                        if self.promoted.contains(&idx) {
                            continue;
                        }
                    }
                    self.rewritten[block_idx].push(stmt.clone());
                }
                StmtInner::Store(s) => {
                    if let Some(ptr_idx) = s.ptr.vreg_index() {
                        if self.promoted.contains(&ptr_idx) {
                            let src = self.resolve_alias(&s.src);
                            if let Some(stack) = self.var_stack.get_mut(&ptr_idx) {
                                stack.push(src.clone());
                                pushed_vars.push(ptr_idx);
                            }
                            continue;
                        }
                    }
                    let rewritten = self.rewrite_stmt(stmt);
                    self.rewritten[block_idx].push(rewritten);
                }
                StmtInner::Load(s) => {
                    if let Some(ptr_idx) = s.ptr.vreg_index() {
                        if self.promoted.contains(&ptr_idx) {
                            if let Some(dst_idx) = s.dst.vreg_index() {
                                let cur = self.current_value(ptr_idx);
                                self.alias_map.insert(dst_idx, cur);
                                added_aliases.push(dst_idx);
                            }
                            continue;
                        }
                    }
                    let rewritten = self.rewrite_stmt(stmt);
                    self.rewritten[block_idx].push(rewritten);
                }
                _ => {
                    let rewritten = self.rewrite_stmt(stmt);
                    self.rewritten[block_idx].push(rewritten);
                }
            }
        }

        let pred_label = self.cfg.label(block_idx).clone();
        for &succ in self.cfg.successors(block_idx) {
            let incoming_vals: Vec<Operand> = self
                .phi_placement
                .phis_at(succ)
                .iter()
                .map(|phi| self.current_value(phi.var))
                .collect();

            for (phi, val) in self
                .phi_placement
                .phis_at_mut(succ)
                .iter_mut()
                .zip(incoming_vals)
            {
                phi.incomings.push((pred_label.clone(), val));
            }
        }

        let children: Vec<usize> = self.dom_info.dom_children(block_idx).to_vec();
        for child in children {
            self.rename_block(child);
        }

        for idx in added_aliases {
            self.alias_map.remove(&idx);
        }
        for var in pushed_vars.into_iter().rev() {
            if let Some(stack) = self.var_stack.get_mut(&var) {
                stack.pop();
            }
        }
    }

    fn clear_state(&mut self) {
        for stack in self.var_stack.values_mut() {
            stack.clear();
        }
        self.alias_map.clear();
    }

    fn current_value(&self, var: usize) -> Operand {
        self.var_stack
            .get(&var)
            .and_then(|stack| stack.last())
            .map(|v| self.resolve_alias(v))
            .unwrap_or_else(|| Operand::from(0))
    }

    fn resolve_alias(&self, op: &Operand) -> Operand {
        let mut cur = op.clone();
        loop {
            if let Operand::Local(l) = &cur {
                if let Some(next) = self.alias_map.get(&l.index) {
                    cur = next.clone();
                    continue;
                }
            }
            break;
        }
        cur
    }

    fn rewrite_stmt(&self, stmt: &Stmt) -> Stmt {
        stmt.map_use_operands(|op| self.resolve_alias(op))
    }
}
