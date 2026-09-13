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

#[cfg(test)]
mod tests {
    use super::super::compile::{BarrierKind, CompiledGraph, compile};
    use super::super::desc::{LeafPass, TexAccess, TexId};
    use super::super::test_desc::{DescBuilder, mutate, raster, read, write};
    use super::{ExpandedFrame, FrameShape, LeafRef, ResolvedTex, RunState, TexRunState, expand};

    fn expand_fresh(graph: &CompiledGraph, shape: &FrameShape) -> ExpandedFrame {
        expand(graph, &RunState::new(&graph.tex_phys), shape)
    }

    fn compiled(builder: &DescBuilder) -> CompiledGraph {
        let analysis = builder.validate().expect("desc under test must validate");

        compile(&builder.desc, &analysis, &builder.schemas)
    }

    #[test]
    fn prev_phys_is_the_other_image() {
        let mut run = TexRunState::new(2);

        assert_eq!(run.read_phys(), 0);
        assert_eq!(run.prev_phys(), 1);
        run.commit_write();
        assert_eq!(run.read_phys(), 1);
        assert_eq!(run.prev_phys(), 0);
    }

    /// with one image there is no previous version to point at
    #[test]
    fn single_image_cursor_never_moves() {
        let mut run = TexRunState::new(1);
        for _ in 0..3 {
            assert_eq!(run.read_phys(), 0);
            assert_eq!(run.prev_phys(), 0);
            assert_eq!(run.write_phys(), 0);
            run.commit_write();
        }

        assert_eq!(run.cursor, 0);
    }

    #[test]
    fn two_image_cursor_alternates_per_write() {
        let mut run = TexRunState::new(2);
        run.commit_write();
        assert_eq!(run.cursor, 1);
        run.commit_write();
        assert_eq!(run.cursor, 0);
    }

    /// the cursor is graph state, not frame state: an odd write count leaves
    /// the next frame reading the other image
    #[test]
    fn odd_write_counts_carry_across_frames() {
        let mut run = TexRunState::new(2);
        for _ in 0..3 {
            run.commit_write();
        }

        assert_eq!(run.cursor, 1);
        assert_eq!(run.read_phys(), 1);
    }

    #[test]
    fn zero_trip_repeat_emits_nothing_and_keeps_cursors() {
        let mut builder = DescBuilder::new(2);
        let body = builder.dispatch_with_push("jacobi", &[read(1)], &[read(0), write(0)]);
        builder.repeat("repeat0", vec![LeafPass::Compute(body)]);
        let graph = compiled(&builder);
        let state = RunState::new(&graph.tex_phys);
        let frame = expand(&graph, &state, &FrameShape::neutral(graph.value_count));

        assert!(frame.steps.is_empty());
        assert_eq!(frame.next, state);
    }

    #[test]
    fn odd_repeat_flips_cursor_parity() {
        let mut builder = DescBuilder::new(2);
        let body = builder.dispatch_with_push("jacobi", &[read(1)], &[read(0), write(0)]);
        let count = builder.repeat("repeat0", vec![LeafPass::Compute(body)]);
        let graph = compiled(&builder);
        let mut shape = FrameShape::neutral(graph.value_count);
        shape.counts[count.0 as usize] = 3;
        let frame = expand_fresh(&graph, &shape);

        assert_eq!(frame.steps.len(), 3);
        assert_eq!(frame.next.tex[0].cursor, 1);
    }

    #[test]
    fn even_repeat_restores_parity() {
        let mut builder = DescBuilder::new(2);
        let body = builder.dispatch_with_push("jacobi", &[read(1)], &[read(0), write(0)]);
        let count = builder.repeat("repeat0", vec![LeafPass::Compute(body)]);
        let graph = compiled(&builder);
        let mut shape = FrameShape::neutral(graph.value_count);
        shape.counts[count.0 as usize] = 4;
        let frame = expand_fresh(&graph, &shape);

        assert_eq!(frame.steps.len(), 4);
        assert_eq!(frame.next.tex[0].cursor, 0);
    }

    /// every resolved physical index matches a hand replay of the same access
    /// sequence
    #[test]
    fn cursor_parity_matches_tex_run_state_replay() {
        let mut builder = DescBuilder::new(2);
        let body = builder.dispatch_with_push("jacobi", &[read(1)], &[read(0), write(0)]);
        let count = builder.repeat("repeat0", vec![LeafPass::Compute(body)]);
        let graph = compiled(&builder);
        assert_eq!(graph.tex_phys, vec![2, 1]);
        let mut shape = FrameShape::neutral(graph.value_count);
        shape.counts[count.0 as usize] = 3;
        let frame = expand_fresh(&graph, &shape);

        let mut rotating = TexRunState::new(2);
        let stable = TexRunState::new(1);
        for step in &frame.steps {
            assert_eq!(
                step.tex,
                vec![
                    ResolvedTex {
                        tex: TexId(1),
                        phys: stable.read_phys(),
                        access: TexAccess::Read,
                    },
                    ResolvedTex {
                        tex: TexId(0),
                        phys: rotating.read_phys(),
                        access: TexAccess::Read,
                    },
                    ResolvedTex {
                        tex: TexId(0),
                        phys: rotating.write_phys(),
                        access: TexAccess::Write,
                    },
                ]
            );
            rotating.commit_write();
        }

        assert_eq!(frame.next.tex[0], rotating);
    }

    #[test]
    fn skipped_when_emits_nothing_and_keeps_cursors() {
        let mut builder = DescBuilder::new(1);
        let body = builder.dispatch("gated", &[read(0), write(0)]);
        let gate = builder.when("when0", vec![LeafPass::Compute(body)]);
        let graph = compiled(&builder);
        let state = RunState::new(&graph.tex_phys);
        let mut shape = FrameShape::neutral(graph.value_count);
        shape.present[gate.0 as usize] = false;
        let frame = expand(&graph, &state, &shape);

        assert!(frame.steps.is_empty());
        assert_eq!(frame.next, state);
    }

    /// the barrier belongs to the step that runs, not to the pass slot, so a
    /// skipped body does not drop the barrier between its neighbours
    #[test]
    fn skipped_when_keeps_barriers_between_survivors() {
        let mut builder = DescBuilder::new(1);
        let first = builder.dispatch("a", &[read(0)]);
        builder.leaf(first);
        let gated = builder.dispatch("b", &[read(0)]);
        let gate = builder.when("when0", vec![LeafPass::Compute(gated)]);
        let last = builder.dispatch("c", &[read(0)]);
        builder.leaf(last);
        let graph = compiled(&builder);
        let mut shape = FrameShape::neutral(graph.value_count);
        shape.present[gate.0 as usize] = false;
        let frame = expand_fresh(&graph, &shape);

        assert_eq!(frame.steps.len(), 2);
        assert_eq!(frame.steps[0].barrier_before, None);
        assert_eq!(frame.steps[0].leaf.pass, 0);
        assert_eq!(
            frame.steps[1].barrier_before,
            Some(BarrierKind::ComputeSync)
        );
        assert_eq!(frame.steps[1].leaf.pass, 2);
    }

    #[test]
    fn first_executed_step_has_no_barrier() {
        let mut builder = DescBuilder::new(1);
        let gated = builder.dispatch("a", &[read(0)]);
        builder.when("when0", vec![LeafPass::Compute(gated)]);
        let last = builder.dispatch("b", &[read(0)]);
        builder.leaf(last);
        let graph = compiled(&builder);
        let frame = expand_fresh(&graph, &FrameShape::neutral(graph.value_count));

        assert_eq!(frame.steps.len(), 2);
        assert_eq!(frame.steps[0].barrier_before, None);
        assert_eq!(
            frame.steps[0].leaf,
            LeafRef {
                pass: 0,
                body_index: 0,
                iteration: 0,
            }
        );
        assert_eq!(
            frame.steps[1].barrier_before,
            Some(BarrierKind::ComputeSync)
        );
    }

    #[test]
    fn raster_only_graph_has_no_leading_barrier() {
        let mut builder = DescBuilder::new(1);
        let draw = builder.draw("draw0", &[read(0)]);
        let pass = raster("main", vec![draw]);
        builder.leaf_raster(pass);
        let graph = compiled(&builder);
        let frame = expand_fresh(&graph, &FrameShape::neutral(graph.value_count));

        assert_eq!(frame.steps.len(), 1);
        assert_eq!(frame.steps[0].barrier_before, None);
    }

    #[test]
    fn input_run_state_is_not_mutated() {
        let mut builder = DescBuilder::new(1);
        let body = builder.dispatch("d0", &[read(0), write(0)]);
        builder.leaf(body);
        let graph = compiled(&builder);
        let state = RunState::new(&graph.tex_phys);
        let first = expand(&graph, &state, &FrameShape::neutral(graph.value_count));
        let second = expand(&graph, &state, &FrameShape::neutral(graph.value_count));

        assert_eq!(state.tex[0].cursor, 0);
        assert_eq!(first.next, second.next);
        assert_eq!(first.steps[0].tex, second.steps[0].tex);
    }

    /// a mutate edits the current version in place; only a write produces the
    /// next one
    #[test]
    fn mutate_does_not_advance_cursor_write_does() {
        let mut builder = DescBuilder::new(1);
        let mutator = builder.dispatch("d0", &[mutate(0)]);
        builder.leaf(mutator);
        let writer = builder.dispatch("d1", &[read(0), write(0)]);
        builder.leaf(writer);
        let graph = compiled(&builder);
        assert_eq!(graph.tex_phys, vec![2]);
        let frame = expand_fresh(&graph, &FrameShape::neutral(graph.value_count));

        assert_eq!(
            frame.steps[0].tex,
            vec![ResolvedTex {
                tex: TexId(0),
                phys: 0,
                access: TexAccess::Mutate,
            }]
        );
        assert_eq!(
            frame.steps[1].tex,
            vec![
                ResolvedTex {
                    tex: TexId(0),
                    phys: 0,
                    access: TexAccess::Read,
                },
                ResolvedTex {
                    tex: TexId(0),
                    phys: 1,
                    access: TexAccess::Write,
                },
            ]
        );
        assert_eq!(frame.next.tex[0].cursor, 1);
    }
}
