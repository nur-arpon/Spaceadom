## What does this change?

<!-- One or two sentences: what it does and why. -->

## How was it tested?

<!-- Spaceadom's hard-won testing rules (CLAUDE.md → "Testing laws") apply:
     an untested claim of "fixed" isn't accepted here either. What did you
     actually run, and what did you observe? -->

## Checklist

- [ ] `cargo test --lib` passes (from `src-tauri`)
- [ ] `cargo clippy` is clean
- [ ] `tsc` / `npm run build` is clean
- [ ] If this fixes a non-obvious bug, I added a dated entry to
      `PROJECT_STATUS.md` and, if it's a real root-cause fix, an entry to
      `V14_FIXES_AND_CODE.md` (symptom → root cause → file → code → how it was
      verified) — see CLAUDE.md → "Every solved problem gets TWO entries."
- [ ] I have not deleted, renamed, or overwritten a file I didn't create in
      this change without calling it out explicitly in this description.

## Licensing

By submitting this pull request, you agree that your contribution is
licensed to the project under the terms in [LICENSE](../LICENSE), so that
official builds distributed under that licence can include it.
