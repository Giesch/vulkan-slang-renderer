//! Consuming phase tokens shared by Vulkan submission and failure-injection tests.
pub(super) struct BeforeWait;

pub(super) struct AfterWait(());

pub(super) struct AfterWrite(());

pub(super) struct AfterSubmit(());

impl BeforeWait {
    pub(super) fn wait<E>(self, action: impl FnOnce() -> Result<(), E>) -> Result<AfterWait, E> {
        action()?;

        Ok(AfterWait(()))
    }
}

impl AfterWait {
    pub(super) fn write(self, action: impl FnOnce()) -> AfterWrite {
        action();

        AfterWrite(())
    }
}

impl AfterWrite {
    pub(super) fn submit<E>(
        self,
        action: impl FnOnce() -> Result<(), E>,
        commit: impl FnOnce(),
    ) -> Result<AfterSubmit, E> {
        action()?;
        commit();

        Ok(AfterSubmit(()))
    }
}

impl AfterSubmit {
    pub(super) fn present<T, E>(self, action: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        action()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn run(fail: &str) -> (Result<(), ()>, Vec<&'static str>) {
        let events = RefCell::new(Vec::new());
        let action = |phase| {
            events.borrow_mut().push(phase);
            let failed = fail == phase;

            if failed { Err(()) } else { Ok(()) }
        };
        let result = (|| {
            let waited = BeforeWait.wait(|| action("wait"))?;
            // Acquisition and recording remain between these phases in production.
            action("acquire")?;
            let written = waited.write(|| events.borrow_mut().push("write"));
            action("record")?;
            let submitted =
                written.submit(|| action("submit"), || events.borrow_mut().push("commit"))?;

            submitted.present(|| action("present"))
        })();

        (result, events.into_inner())
    }

    #[test]
    fn renderer_adapter_submission_order() {
        let (result, events) = run("");
        assert!(result.is_ok());
        assert_eq!(
            events,
            [
                "wait", "acquire", "write", "record", "submit", "commit", "present"
            ]
        );
    }

    #[test]
    fn renderer_adapter_no_commit_before_submit() {
        for phase in ["wait", "acquire", "record", "submit"] {
            let (result, events) = run(phase);
            assert!(result.is_err());
            assert!(!events.contains(&"commit"));
            assert!(!events.contains(&"present"));
            let failed_before_write = phase == "wait" || phase == "acquire";
            if failed_before_write {
                assert!(!events.contains(&"write"));
            }
        }
    }

    #[test]
    fn renderer_adapter_commit_before_present_error() {
        let (result, events) = run("present");
        assert!(result.is_err());
        assert_eq!(events.iter().filter(|&&event| event == "commit").count(), 1);
        assert_eq!(&events[events.len() - 2..], ["commit", "present"]);
    }
}
