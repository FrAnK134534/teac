use super::types::Dtype;
use std::fmt::{Display, Formatter};

pub trait Typed {
    fn dtype(&self) -> &Dtype;
}

pub trait Named {
    fn identifier(&self) -> Option<String>;
}

#[derive(Clone)]
pub enum Operand {
    Integer(Integer),
    Local(LocalRef),
    Global(GlobalVariable),
}

impl Operand {
    pub fn dtype(&self) -> &Dtype {
        match self {
            Operand::Integer(i) => i.dtype(),
            Operand::Local(l) => l.dtype(),
            Operand::Global(g) => g.dtype(),
        }
    }

    pub fn as_local(&self) -> Option<&LocalRef> {
        match self {
            Operand::Local(l) => Some(l),
            _ => None,
        }
    }

    pub fn is_addressable(&self) -> bool {
        matches!(self, Operand::Local(_) | Operand::Global(_))
    }

    pub fn vreg_index(&self) -> Option<usize> {
        self.as_local().map(|l| l.index)
    }
}

impl Display for Operand {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Operand::Integer(i) => write!(f, "{}", i),
            Operand::Local(l) => write!(f, "{}", l),
            Operand::Global(g) => write!(f, "{}", g),
        }
    }
}

impl From<Integer> for Operand {
    fn from(i: Integer) -> Self {
        Operand::Integer(i)
    }
}

impl From<LocalVariable> for Operand {
    fn from(l: LocalVariable) -> Self {
        Operand::Local(LocalRef::from(l))
    }
}

impl From<&LocalVariable> for Operand {
    fn from(l: &LocalVariable) -> Self {
        Operand::Local(LocalRef::from(l))
    }
}

impl From<GlobalVariable> for Operand {
    fn from(g: GlobalVariable) -> Self {
        Operand::Global(g)
    }
}

impl From<i32> for Operand {
    fn from(v: i32) -> Self {
        Operand::Integer(Integer::from(v))
    }
}

#[derive(Clone)]
pub struct Integer {
    pub value: i32,
}

impl From<i32> for Integer {
    fn from(value: i32) -> Self {
        Self { value }
    }
}

impl Typed for Integer {
    fn dtype(&self) -> &Dtype {
        &Dtype::I32
    }
}

impl Named for Integer {
    fn identifier(&self) -> Option<String> {
        None
    }
}

impl Display for Integer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

#[derive(Clone)]
pub struct LocalVariable {
    pub dtype: Dtype,
    pub identifier: Option<String>,
    pub index: usize,
}

#[derive(Clone)]
pub struct LocalRef {
    pub dtype: Dtype,
    pub index: usize,
}

impl From<LocalVariable> for LocalRef {
    fn from(value: LocalVariable) -> Self {
        Self {
            dtype: value.dtype,
            index: value.index,
        }
    }
}

impl From<&LocalVariable> for LocalRef {
    fn from(value: &LocalVariable) -> Self {
        Self {
            dtype: value.dtype.clone(),
            index: value.index,
        }
    }
}

impl Typed for LocalRef {
    fn dtype(&self) -> &Dtype {
        &self.dtype
    }
}

impl Display for LocalRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "%r{}", self.index)
    }
}

impl Typed for LocalVariable {
    fn dtype(&self) -> &Dtype {
        &self.dtype
    }
}

impl Named for LocalVariable {
    fn identifier(&self) -> Option<String> {
        self.identifier.clone()
    }
}

impl Display for LocalVariable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "%r{}", self.index)
    }
}

impl LocalVariable {
    pub fn new(dtype: Dtype, index: usize, identifier: Option<String>) -> Self {
        Self {
            dtype,
            identifier,
            index,
        }
    }
}

#[derive(Clone)]
pub struct GlobalVariable {
    pub dtype: Dtype,
    pub identifier: String,
    pub initializers: Option<Vec<i32>>,
}

impl Typed for GlobalVariable {
    fn dtype(&self) -> &Dtype {
        &self.dtype
    }
}

impl Named for GlobalVariable {
    fn identifier(&self) -> Option<String> {
        Some(self.identifier.clone())
    }
}

impl Display for GlobalVariable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "@{}", self.identifier)
    }
}
