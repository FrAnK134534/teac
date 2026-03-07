use crate::ir::function::Function;

pub mod cfg;
mod dominator;
mod mem2reg;

pub use mem2reg::Mem2RegPass;

pub trait FunctionPass {
    fn run(&self, func: &mut Function);
}

#[derive(Default)]
pub struct FunctionPassManager {
    passes: Vec<Box<dyn FunctionPass>>,
}

impl FunctionPassManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_default_pipeline() -> Self {
        let mut pm = Self::new();
        pm.add_pass(Mem2RegPass);
        pm
    }

    pub fn add_pass<P>(&mut self, pass: P)
    where
        P: FunctionPass + 'static,
    {
        self.passes.push(Box::new(pass));
    }

    pub fn run(&self, func: &mut Function) {
        for pass in &self.passes {
            pass.run(func);
        }
    }
}
