//! The guard around every request. A panic in the core (a bug, or geometry a library cannot take) is caught, the
//! project is put back as it was before the request, unsaved work is written to a recovery copy, and the request
//! fails with a message. Without it one fault stops the thread the project lives on and the session's work is lost.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Once;

use crate::{Res, Session};

/// The undo steps kept. Older ones are dropped between requests, so a long session's history cannot grow without
/// end.
const UNDO_STEPS: usize = 500;

thread_local! {
    /// What the last panic on this thread said and where, kept by the panic hook for the error message.
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Keeps each panic's message and place, then hands it to the hook that was there before (which prints it).
fn note_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let said = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".to_string());
            let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
            let _ = LAST_PANIC.try_with(|last| *last.borrow_mut() = Some(format!("{said}{at}")));
            before(info);
        }));
    });
}

impl Session {
    pub(crate) fn guarded(&mut self, method: &str, run: impl FnOnce(&mut Session) -> Res) -> Res {
        note_panics();
        self.doc.trim_undo(UNDO_STEPS);
        let (mark, serial) = (self.doc.edit_mark(), self.doc_serial);
        let _ = LAST_PANIC.try_with(|last| last.borrow_mut().take());
        match catch_unwind(AssertUnwindSafe(|| run(self))) {
            Ok(answer) => answer,
            Err(_) => {
                let detail = LAST_PANIC.try_with(|last| last.borrow_mut().take()).ok().flatten().unwrap_or_else(|| "no detail".to_string());
                // a request that had already swapped in another project (New, Open) has no steps of this one to undo
                let mark = if serial == self.doc_serial { mark } else { usize::MAX };
                let tidy = catch_unwind(AssertUnwindSafe(|| self.doc.recover_after_panic(mark))).is_ok();
                let copy = if tidy { self.write_recovery_copy() } else { None };
                let mut text = format!("OpenArch BIM hit an internal error in \"{method}\" and put the project back as it was before that step.");
                if let Some(path) = copy {
                    text.push_str(&format!(" A copy of your unsaved work was written to {}.", path.display()));
                }
                text.push_str(&format!(" Save your work and restart the program. ({detail})"));
                Err(text)
            }
        }
    }

    /// Writes the project to a copy beside its file (or in the temporary folder when it has no file yet), when it
    /// has unsaved changes that no earlier copy holds. The project's own file is left alone.
    fn write_recovery_copy(&mut self) -> Option<PathBuf> {
        let revision = self.doc.revision();
        if revision == self.saved_revision {
            return None;
        }
        let path = match &self.path {
            Some(p) => p.with_extension("recovered.oab"),
            None => std::env::temp_dir().join("OpenArch BIM").join("Untitled.recovered.oab"),
        };
        if self.recovered_revision == Some(revision) {
            return Some(path);
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let saved = catch_unwind(AssertUnwindSafe(|| self.doc.save(&path))).ok()?;
        saved.ok()?;
        self.recovered_revision = Some(revision);
        Some(path)
    }
}
