use super::error::Error;
use super::function::{BasicBlock, Function};
use super::types::{Dtype, FunctionType, StructType};
use super::value::GlobalVariable;
use std::io::Write;

pub struct IrPrinter<W: Write> {
    writer: W,
}

impl<W: Write> IrPrinter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn emit_header(&mut self, target_triple: &str, datalayout: &str) -> Result<(), Error> {
        writeln!(self.writer, "target triple = \"{}\"", target_triple)?;
        writeln!(self.writer, "target datalayout = \"{}\"", datalayout)?;
        writeln!(self.writer)?;
        Ok(())
    }

    pub fn emit_struct_type(&mut self, name: &str, st: &StructType) -> Result<(), Error> {
        let members: Vec<String> = st
            .elements
            .iter()
            .map(|e| format!("{}", e.1.dtype))
            .collect();
        let members = members.join(", ");
        writeln!(self.writer, "%{} = type {{ {} }}", name, members)?;
        Ok(())
    }

    pub fn emit_global(&mut self, global: &GlobalVariable) -> Result<(), Error> {
        let init_str = match (&global.initializers, &global.dtype) {
            (None, Dtype::I32) => "0".to_string(),
            (None, _) => "zeroinitializer".to_string(),
            (Some(inits), Dtype::Array { element, .. }) => {
                let elems: Vec<String> =
                    inits.iter().map(|v| format!("{} {}", element, v)).collect();
                format!("[{}]", elems.join(", "))
            }
            (Some(inits), _) if inits.len() == 1 => {
                format!("{}", inits[0])
            }
            (Some(inits), _) => {
                let elems: Vec<String> = inits.iter().map(|v| format!("i32 {}", v)).collect();
                format!("[{}]", elems.join(", "))
            }
        };

        writeln!(
            self.writer,
            "@{} = dso_local global {} {}, align 4",
            global.identifier, global.dtype, init_str
        )?;
        Ok(())
    }

    pub fn emit_function_def(
        &mut self,
        func: &Function,
        return_dtype: &Dtype,
        blocks: &[BasicBlock],
    ) -> Result<(), Error> {
        let args = func
            .arguments
            .iter()
            .map(|var| {
                if matches!(&var.dtype, Dtype::Pointer { .. }) {
                    format!("ptr %r{}", var.index)
                } else {
                    format!("{} %r{}", var.dtype, var.index)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        writeln!(
            self.writer,
            "define dso_local {} @{}({}) {{",
            return_dtype, func.identifier, args
        )?;
        for block in blocks {
            writeln!(self.writer, "{}:", block.label)?;
            for stmt in &block.stmts {
                writeln!(self.writer, "{}", stmt)?;
            }
        }
        writeln!(self.writer, "}}")?;
        writeln!(self.writer)?;
        Ok(())
    }

    pub fn emit_function_decl(
        &mut self,
        identifier: &str,
        func_type: &FunctionType,
    ) -> Result<(), Error> {
        let args = func_type
            .arguments
            .iter()
            .map(|(_, dtype)| format!("{}", dtype))
            .collect::<Vec<_>>()
            .join(", ");

        writeln!(
            self.writer,
            "declare dso_local {} @{}({})",
            func_type.return_dtype, identifier, args
        )?;
        writeln!(self.writer)?;
        Ok(())
    }

    pub fn emit_newline(&mut self) -> Result<(), Error> {
        writeln!(self.writer)?;
        Ok(())
    }
}
