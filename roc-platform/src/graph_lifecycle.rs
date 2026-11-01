//! Shared setup/dispatch orchestration. Sources and adapters can be replaced in
//! CPU tests without replacing the production control flow.
use anyhow::Context;

pub(crate) struct GraphLifecycle<P> {
    graphs: Vec<P>,
}

impl<P> GraphLifecycle<P> {
    pub(crate) fn setup<D>(
        config: impl FnOnce() -> Vec<D>,
        mut prepare: impl FnMut(D) -> anyhow::Result<P>,
    ) -> anyhow::Result<Self> {
        let graphs = config()
            .into_iter()
            .enumerate()
            .map(|(id, definition)| {
                prepare(definition).with_context(|| format!("configured graph {id}"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self { graphs })
    }

    pub(crate) fn frame<V, E>(
        &mut self,
        draw: impl FnOnce() -> (u32, V),
        execute: impl FnOnce(&mut P, V) -> Result<(), E>,
    ) -> Result<(), E>
    where
        E: From<anyhow::Error>,
    {
        let (id, values) = draw();
        let graph = self
            .graphs
            .get_mut(id as usize)
            .with_context(|| format!("Roc selected unknown graph {id}"))?;
        execute(graph, values)
    }
}

#[cfg(test)]
mod tests {
    use super::GraphLifecycle;
    use std::cell::Cell;

    #[test]
    fn setup_prepares_config_without_draw() {
        let config_calls = Cell::new(0);
        let draw_calls = Cell::new(0);
        let mut prepared = Vec::new();
        let mut lifecycle = GraphLifecycle::setup(
            || {
                config_calls.set(config_calls.get() + 1);
                vec![10, 20, 30]
            },
            |definition| {
                prepared.push(definition);
                Ok(definition)
            },
        )
        .unwrap();
        assert_eq!(config_calls.get(), 1);
        assert_eq!(prepared, [10, 20, 30]);
        assert_eq!(draw_calls.get(), 0);
        for (id, expected, payload) in [(2, 30, vec![3]), (0, 10, vec![1, 2])] {
            lifecycle
                .frame(
                    || {
                        draw_calls.set(draw_calls.get() + 1);
                        (id, payload.clone())
                    },
                    |graph, values| -> anyhow::Result<()> {
                        assert_eq!(*graph, expected);
                        assert_eq!(values, payload);
                        Ok(())
                    },
                )
                .unwrap();
        }
        assert_eq!(config_calls.get(), 1);
        assert_eq!(draw_calls.get(), 2);
    }

    #[test]
    fn frame_id_out_of_range() {
        let mut lifecycle = GraphLifecycle::setup(|| vec![0], Ok).unwrap();
        let result: anyhow::Result<()> = lifecycle.frame(
            || (1, vec![1]),
            |_, _| panic!("out-of-range graph must not execute"),
        );
        assert!(result.unwrap_err().to_string().contains("unknown graph 1"));
    }

    #[test]
    fn partial_setup_drops_prepared_graphs() {
        struct Prepared<'a>(&'a Cell<usize>);
        impl Drop for Prepared<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Cell::new(0);
        let result = GraphLifecycle::setup(
            || vec![0, 1, 2],
            |id| {
                if id == 2 {
                    anyhow::bail!("prepare failed");
                }
                Ok(Prepared(&drops))
            },
        );
        assert!(result.is_err());
        assert_eq!(drops.get(), 2);
    }
}
