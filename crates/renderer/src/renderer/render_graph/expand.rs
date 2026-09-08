//! Pure per-frame expansion. Phase 1 does not wire this into execution.
use super::compile::*;
use super::desc::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TexRunState {
    pub(crate) phys_count: u32,
    pub(crate) cursor: u32,
}

impl TexRunState {
    pub(crate) fn new(phys_count: u32) -> Self {
        debug_assert!((1..=2).contains(&phys_count));
        Self {
            phys_count,
            cursor: 0,
        }
    }

    pub(crate) fn read_phys(&self) -> u32 {
        self.cursor
    }

    pub(crate) fn prev_phys(&self) -> u32 {
        (self.cursor + 1) % self.phys_count
    }

    pub(crate) fn write_phys(&self) -> u32 {
        (self.cursor + 1) % self.phys_count
    }

    pub(crate) fn commit_write(&mut self) {
        self.cursor = self.write_phys()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunState {
    pub(crate) tex: Vec<TexRunState>,
}

impl RunState {
    pub(crate) fn new(phys_counts: &[u32]) -> Self {
        Self {
            tex: phys_counts.iter().copied().map(TexRunState::new).collect(),
        }
    }
}

pub(crate) struct FrameShape {
    pub(crate) counts: Vec<u32>,
    pub(crate) present: Vec<bool>,
}

impl FrameShape {
    pub(crate) fn neutral(value_count: u32) -> Self {
        Self {
            counts: vec![0; value_count as usize],
            present: vec![true; value_count as usize],
        }
    }
}

pub(crate) struct ExpandedFrame {
    pub(crate) steps: Vec<ExecStep>,
    pub(crate) next: RunState,
}

pub(crate) struct ExecStep {
    pub(crate) barrier_before: Option<BarrierKind>,
    pub(crate) leaf: LeafRef,
    pub(crate) tex: Vec<ResolvedTex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LeafRef {
    pub(crate) pass: u32,
    pub(crate) body_index: u32,
    pub(crate) iteration: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedTex {
    pub(crate) tex: TexId,
    pub(crate) phys: u32,
    pub(crate) access: TexAccess,
}

pub(crate) fn expand(graph: &CompiledGraph, state: &RunState, shape: &FrameShape) -> ExpandedFrame {
    let mut next = state.clone();
    let mut steps = vec![];
    let mut emit = |compiled: &CompiledLeaf, leaf: LeafRef| {
        let mut resolved = vec![];
        for (tex, access) in compiled
            .access
            .reads
            .iter()
            .map(|tex| (tex, TexAccess::Read))
            .chain(
                compiled
                    .access
                    .prev_reads
                    .iter()
                    .map(|tex| (tex, TexAccess::ReadPrevious)),
            )
            .chain(
                compiled
                    .access
                    .mutates
                    .iter()
                    .map(|tex| (tex, TexAccess::Mutate)),
            )
            .chain(
                compiled
                    .access
                    .writes
                    .iter()
                    .map(|tex| (tex, TexAccess::Write)),
            )
        {
            let run = &next.tex[tex.0 as usize];
            let phys = match access {
                TexAccess::Read | TexAccess::Mutate => run.read_phys(),
                TexAccess::ReadPrevious => run.prev_phys(),
                TexAccess::Write => run.write_phys(),
            };
            resolved.push(ResolvedTex {
                tex: *tex,
                phys,
                access,
            })
        }
        for tex in &compiled.access.writes {
            next.tex[tex.0 as usize].commit_write()
        }
        steps.push(ExecStep {
            barrier_before: (!steps.is_empty()).then_some(compiled.barrier_before),
            leaf,
            tex: resolved,
        })
    };

    for (pass_index, pass) in graph.passes.iter().enumerate() {
        match pass {
            CompiledPass::Leaf(leaf) => emit(
                leaf,
                LeafRef {
                    pass: pass_index as u32,
                    body_index: 0,
                    iteration: 0,
                },
            ),
            CompiledPass::When { gate, body } => {
                if shape.present[gate.0 as usize] {
                    for (i, leaf) in body.iter().enumerate() {
                        emit(
                            leaf,
                            LeafRef {
                                pass: pass_index as u32,
                                body_index: i as u32,
                                iteration: 0,
                            },
                        )
                    }
                }
            }
            CompiledPass::Repeat { count, body } => {
                for iteration in 0..shape.counts[count.0 as usize] {
                    for (i, leaf) in body.iter().enumerate() {
                        emit(
                            leaf,
                            LeafRef {
                                pass: pass_index as u32,
                                body_index: i as u32,
                                iteration,
                            },
                        )
                    }
                }
            }
        }
    }

    ExpandedFrame { steps, next }
}
