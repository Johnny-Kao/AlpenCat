//! Backend-neutral internal primitives for the experimental runtime framework.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkRange {
    pub begin: usize,
    pub end: usize,
}

impl WorkRange {
    pub const fn new(begin: usize, end: usize) -> Self {
        Self { begin, end }
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.begin)
    }

    pub const fn is_empty(self) -> bool {
        self.begin >= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    Serial,
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionBudget {
    pub max_parallelism: usize,
}

impl ExecutionBudget {
    pub const fn new(max_parallelism: usize) -> Self {
        Self {
            max_parallelism: if max_parallelism == 0 {
                1
            } else {
                max_parallelism
            },
        }
    }

    pub const fn serial() -> Self {
        Self { max_parallelism: 1 }
    }
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self {
            max_parallelism: std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(1),
        }
    }
}
