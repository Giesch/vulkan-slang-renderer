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
    pub(crate) fn new(p: &[u32]) -> Self {
        Self {
            tex: p.iter().copied().map(TexRunState::new).collect(),
        }
    }
}
pub(crate) struct FrameShape {
    pub(crate) counts: Vec<u32>,
    pub(crate) present: Vec<bool>,
}
impl FrameShape {
    pub(crate) fn neutral(n: u32) -> Self {
        Self {
            counts: vec![0; n as usize],
            present: vec![true; n as usize],
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
pub(crate) fn expand(g: &CompiledGraph, state: &RunState, shape: &FrameShape) -> ExpandedFrame {
    let mut next = state.clone();
    let mut steps = vec![];
    let mut emit = |l: &CompiledLeaf, leaf: LeafRef| {
        let mut tex = vec![];
        for (t, a) in l
            .access
            .reads
            .iter()
            .map(|t| (t, TexAccess::Read))
            .chain(
                l.access
                    .prev_reads
                    .iter()
                    .map(|t| (t, TexAccess::ReadPrevious)),
            )
            .chain(l.access.mutates.iter().map(|t| (t, TexAccess::Mutate)))
            .chain(l.access.writes.iter().map(|t| (t, TexAccess::Write)))
        {
            let s = &next.tex[t.0 as usize];
            let phys = match a {
                TexAccess::Read | TexAccess::Mutate => s.read_phys(),
                TexAccess::ReadPrevious => s.prev_phys(),
                TexAccess::Write => s.write_phys(),
            };
            tex.push(ResolvedTex {
                tex: *t,
                phys,
                access: a,
            })
        }
        for t in &l.access.writes {
            next.tex[t.0 as usize].commit_write()
        }
        steps.push(ExecStep {
            barrier_before: (!steps.is_empty()).then_some(l.barrier_before),
            leaf,
            tex,
        })
    };
    for (pi, p) in g.passes.iter().enumerate() {
        match p {
            CompiledPass::Leaf(l) => emit(
                l,
                LeafRef {
                    pass: pi as u32,
                    body_index: 0,
                    iteration: 0,
                },
            ),
            CompiledPass::When { gate, body } => {
                if shape.present[gate.0 as usize] {
                    for (i, l) in body.iter().enumerate() {
                        emit(
                            l,
                            LeafRef {
                                pass: pi as u32,
                                body_index: i as u32,
                                iteration: 0,
                            },
                        )
                    }
                }
            }
            CompiledPass::Repeat { count, body } => {
                for iteration in 0..shape.counts[count.0 as usize] {
                    for (i, l) in body.iter().enumerate() {
                        emit(
                            l,
                            LeafRef {
                                pass: pi as u32,
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
