
# Systematic Debugging

Two rules. Find the cause before changing anything. Prove it worked before saying it worked.

Most wasted debugging time is not spent finding hard bugs. It is spent applying
plausible fixes to symptoms, declaring victory, and coming back to the same bug
an hour later under a different description.

## The tell you are in the loop

Stop and restart from Phase 1 if any of these are true:

- The same behaviour has been "fixed" before, by a different mechanism.
- The fix is a different way of doing the same thing (one input-simulation API
  swapped for another input-simulation API).
- You are adjusting a number to see what happens.
- You are about to write "reliable", "flawless", "100%", "perfectly", or
  "guaranteed" about something you have not observed working.
- Three fixes have failed. At three, the problem is the layer, not the code.
  Stop fixing and question the approach.

## Phase 1 — Root cause

Do not write a fix during this phase.

- Read the actual error. All of it. Not the first line.
- Reproduce it deliberately. A bug you cannot trigger on demand cannot be
  verified as fixed — you will only be able to observe its absence, which is not
  the same thing.
- Ask what changed. Most bugs are recent.
- Trace backwards from the symptom to where the bad value first existed. The
  symptom is where you noticed it, rarely where it came from.
- Check the boundaries: FFI calls, IPC between frontend and backend, callbacks
  the OS invokes on its own thread, anything crossing a serialization step.

State the cause in one sentence before continuing. If you cannot, you have not
found it, and the next edit is a guess.

## Phase 1b — Distrust the test and the environment

Before believing any failure, confirm the failure is real.

- **Run the test harness against a known-good or switched-off target.** If it
  reports failure with nothing to fail, the harness is broken, not the code. This
  check takes one minute and routinely saves an afternoon.
- **Read the return value of every system call in the harness.** A call that
  silently returns an error code produces results that look exactly like the code
  under test misbehaving. Wrong struct sizes, wrong process, wrong window handle —
  all of these report as "nothing happened".
- **Confirm nothing else is already doing the job.** An older version still
  running from a startup entry, a leftover background process, another tool
  hooking the same input — any of these make every observation meaningless, and
  two of them can feed each other in a loop.
- **A tool that exists is not a tool that works.** Check the file has contents and
  actually runs, not just that the path resolves. Zero-byte shims and stale
  wrappers report "found" and fail with errors that point somewhere else entirely.

State the limits of what automation can prove. Some behaviour depends on real
physical state the OS will not synthesize — genuine key-down state, real focus,
hardware events. If a test cannot reach it, prove that it cannot, write it down,
and ask for a human check. Do not keep rewriting the test.

## Phase 2 — Compare against something that works

- Find a working instance of the same pattern, in this codebase or upstream docs.
- Diff it against the broken one. The answer is usually in the difference.
- Check the version you are reading docs for matches the version installed. Major
  version mismatches produce errors that look like your code is wrong.

## Phase 3 — Hypothesis

One hypothesis, stated out loud, testable, with a predicted observation.

Change one thing. If the result does not match the prediction, the hypothesis was
wrong — say so and form a new one. Do not keep the change and add another on top.
Stacked speculative changes make the next bug unattributable.

## Phase 4 — Fix and prove it

- Fix the cause, not the symptom.
- **Confirm the code you are testing is the code you wrote.** A fix that fails to
  compile, or that a stale bundle or cached build shadows, presents exactly like a
  fix that did not work. Check the build succeeded before drawing any conclusion
  from a test. This one failure mode has cost more hours than any actual bug.
- Observe the fixed behaviour directly. Not "should now work" — run it.
- Confirm you have not broken the neighbours.

## Verification before completion

Never report as done, fixed, verified, or working:

- Anything you have not run.
- Anything whose build you did not confirm succeeded.
- Anything where you only observed the absence of an error rather than the
  presence of correct behaviour.

If it has not been observed working, say what was actually done and what remains
unverified. "Compiles and installs; mute path not tested on this machine" is
useful. "Verified working flawlessly" about an untested path destroys the value
of every previous status report, because now none of them can be trusted.

Write down what failed, not just what worked. A fix log listing only successes
cannot show you that you have tried the same thing three times.

## Native and OS-level patterns

These recur, and each is expensive to rediscover.

**Simulating input is guessing.** `SendInput`, `PostMessage`, `SendMessageW`,
synthetic scan codes and broadcast window messages ask the OS to pretend a user
did something. Drivers, focus state, elevation and per-app handling all change
whether it lands. If a real API exists — the audio endpoint interface for volume,
the window API for window state, the registry for app resolution — use it. Cycling
between input-simulation methods is the same fix wearing different hats.

**Toggles are not states.** A command meaning "flip this" gives you the wrong
result whenever the starting state is not what you assumed. Set the state you
want explicitly.

**The DOM is display, not storage.** Reading a value back out of a rendered
element to decide what to save gives you whatever was last rendered, not what the
user chose. Keep explicit state with three distinguishable cases: untouched,
explicitly cleared, and set to a value. Collapsing "cleared" and "untouched" into
one falsy value is the bug.

**Your own process is in the enumeration.** Window and process loops include the
app doing the enumerating. Exclude your own PID before acting.

**Injected events re-enter your hook.** A low-level hook that synthesizes input
will see its own synthetic events. Filter them out by checking the `dwExtraInfo`
signature you set on your own `SendInput` calls — NEVER by `LLKHF_INJECTED`,
which also drops Remote Desktop, macro keyboards, and accessibility input
(see `win32-keyboard-hook.md` §1) — or the state machine corrupts itself.

**Every state machine needs its exits audited.** An early return or a mode switch
that skips the normal teardown path leaves flags set. When behaviour is correct
until some other feature is used, look for the path that exits without resetting.

**Ask whether the thing exists before debugging how it looks.** A component that
is built, styled and packaged correctly still shows nothing if nobody ever
created the window or registered the handler that hosts it. Messages sent to a
target that was never instantiated vanish without error. Before inspecting layout,
CSS or rendering, confirm the container exists at runtime and the message arrived.
Repeated rounds of styling changes that fix nothing are the signature of this.

**Permissions files fail silently.** Capability and allowlist configs that omit a
window or a command produce no error — the call simply never lands. When something
works in one window and not another, compare their permissions before their code.

**A strange workaround is evidence, not mess.** Code that forces something open,
retries oddly, or fires an extra event usually exists because someone hit the real
bug and could not see it. Deleting it as careless leftover discards the only clue
and reintroduces the symptom. Work out what it compensates for first, then remove
it along with the cause.

**Transparency and compositing are fragile.** Global stylesheets paint windows
meant to be transparent. Backdrop blur on a transparent layer produces artefacts
on Windows. Suspect the compositor before suspecting your layout.

## Cosmetic changes are not debugging

Adjusting a size, then reverting it, then adjusting it again is not convergence.
Decide the value from a rule — a spacing scale, a container measurement, a
readable minimum — apply it once, and look at it. If there is no rule, that is the
thing to establish, and it is a design task, not a debugging one.

## Scope discipline

Fix the bug you are on. A drive-by refactor in the same change makes it
impossible to tell which edit caused the next symptom. Note the other thing and
come back.

Where a project states features as non-negotiable, a fix that removes or
simplifies one is not a fix. Broken and present beats absent and tidy — repair it
at the level it lives on.
