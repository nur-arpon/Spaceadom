# IF SHORTCUTS DIE AGAIN — READ THIS FIRST

Paste this whole file to any AI before it starts "investigating" why Spaceadom's
shortcuts stop working. This exact bug was found, root-caused, and fixed once
already (PROBLEM 230, fixed in build 1.0.96, 2026-09-04). Three plausible-sounding
explanations were tried first and all three were wrong — the numbers that
disprove them are below so nobody has to re-run those tests. Do not let an AI
skip this file and start guessing from scratch; that is exactly what burns hours
re-solving a solved problem.

## What you will see

Space-held shortcuts (hold Space, tap a letter to launch/focus/minimize an app)
randomly stop responding partway through a hold — the app just goes deaf to the
keyboard for a while, especially noticeable while the Spaceadom dashboard window
itself has focus. Nothing crashes. Nothing appears in the UI. It comes back on
its own after a few seconds, or after the app restarts.

**IF THE RING STOPS APPEARING ENTIRELY AND ONLY A RESTART CURES IT, LOOK FOR A
LATCHED HOLD.** That is a different shape from everything else in this file:
not "shortcuts died mid-press" and not "a ring got stuck on screen" — *no ring
at all, from either witness, until the process is restarted.* The cause is a
`MODIFIER_ACTIVE` that never got its Space-UP, because the keyboard hook was
evicted mid-hold. Guard 2 of the own-window fallback refuses to raise a ring
while that latch is set, so the page-side path is wedged shut too. Grep, in
this order:

```bash
grep "WATCHDOG alarm confirmed, but a Space hold is LIVE" debug.log   # more than ONE per hold = the bug
grep "stale-hold-reaped-because-the-keyboard-is-proven-deaf-spaceadom" debug.log
grep "modifier-active-latched-past-the-bound" debug.log
grep "repair-tore-down-a-hold-that-predated-it-spaceadom" debug.log
```

`Holds protected this session:` climbing 1, 2, 3, 4 for a single hold is the
fingerprint. The three markers below it are the three bounds that now break the
latch; if none of them appears and the ring is still gone, the latch is being
held by something new. Full account: **PROBLEM 262** in
`V14_FIXES_AND_CODE.md`, fixed 2026-09-07 after 1.0.106.

## What it is NOT (three hypotheses already tried and disproved, with the numbers)

**1. "The dashboard's animation is too heavy and starves the keyboard hook of CPU."**
Disproved. Measured live with both app windows minimised and idle: the whole
Spaceadom process tree (including its browser-engine helper windows) used about
1.0 of the machine's 16 CPU cores — **6.3% of the machine** — while the machine
overall sat at 13–20% total load. That is nowhere near enough to starve a
thread. The hook thread itself was also confirmed to be running at a boosted
priority and simply waiting for its turn, not stuck behind other work.

**2. "It's PowerToys or another keyboard-remapping program fighting for the keyboard."**
Disproved, and backwards. Over roughly 17 hours of logs split into windows with
PowerToys/spacedesk closed versus running, the "deafness" rate was **worse with
them closed** — 21.0 deaf-minutes per 100 active minutes with them closed,
versus 9.9 with them running. If they were the cause, closing them should have
helped. It didn't.

**3. "It only happens when our own dashboard window has focus."**
Disproved. In the same log data, the times the dashboard held the foreground
were **42%** of the observed windows, and the times a false "deaf" alarm
happened while it had focus were **40%** — statistically the same rate as
everywhere else. If focus were the cause, the alarm rate while focused should
be much higher than the overall rate. It isn't.

## What it IS

Spaceadom listens to the keyboard using a Windows feature called a "low-level
keyboard hook." To make sure that hook hasn't silently died (Windows is allowed
to kill a hook without telling anyone, if it's ever too slow), the app installs
a second, do-nothing "witness" hook whose only job is to prove the real one is
still alive.

Windows keeps hooks in a list and calls the **most recently installed one
first**. Each hook then has to explicitly hand off to the next one in line
before Windows moves on — and that hand-off is not instant from Windows's point
of view: the first hook's "am I too slow?" timer keeps running for everything
underneath it too. So whichever hook was installed **last** is treated as the
**slowest** hook in the whole chain, because its timer includes everyone below
it. Windows kills whichever hook looks slowest.

The bug: the witness hook was being installed **last**, which put it **first**
in line, which made it look like the slowest hook in the entire chain — so
Windows killed the witness, not the real keyboard hook. Once the witness was
dead, it could only ever report "everything's quiet" (because it wasn't running
to see anything else), so Spaceadom's own internal health-check believed the
real hook had gone silent too, even though it hadn't. That health-check then
tore the real, perfectly-working hook down and reinstalled it — over and over.
Measured on the owner's machine: **514 of these false teardowns in a single
3.8-hour session.** Every one of those is a moment where a shortcut being held
could die mid-press.

## The fix, in the code

File: `src-tauri/src/hook/mod.rs`
Function: `install_hooks()`

The fix is entirely about **order**. The witness/reference hook
(`ref_kb_hook_proc`) is now installed **first**, so it lands at the back of the
line — behind the real keyboard hook (`kb_hook_proc`) and the mouse hook
(`ms_hook_proc`), which are installed after it. That makes the witness the
*fastest-looking* hook (nothing runs under it), so it's the last thing Windows
would ever consider killing.

A second, smaller bug was fixed in the same pass: the witness hook's
install result was never checked, so a failed install and a successfully
installed-then-silently-evicted witness looked identical in the logs (nothing
at all). The code now logs a warning if the witness fails to install.

**DO NOT move the witness hook back to being installed after the real hooks.
DO NOT "clean up" or reorder `install_hooks()` without re-reading this file
first.** That single reordering is the entire fix.

## 2026-09-04 FOLLOW-UP — the symptom came back on 1.0.96, and it was NOT this bug

Read this before you re-open anything above. On 2026-09-04 the owner reported
the same symptom on 1.0.96 — the build that contains the fix. It was measured
against the live log of the running process and **PROBLEM 230 was holding**:
the witness counter kept climbing (6 → 395 → 484 → 588 → 730), and at
`16:59:39.641` the log printed the primary's silence and the witness's silence
as *the same number* (`kb 7922ms` / `ref … 7922ms ago`), which a dying witness
cannot do. Sixteen alarms in 38 minutes, **none** of them the witness-detected
kind, **zero** DEAF lines.

The remaining fault is a different one, written up as **PROBLEM 236**: the
`both_dead` alarm ("user active but NEITHER hook saw anything") is decided from
two clocks that the repair itself and the watchdog's own idle path both write,
so it fires on an ordinary pause in mouse movement — and each false alarm
re-hooks, which clears the Space latch and hides the ring, killing a hold in
progress. That is the "dies mid-press" the owner sees now.

**DECIDED THE SAME DAY, and this is the rule in force now:** an alarm fires only when the keyboard, mouse AND reference CALLBACK clocks are ALL silent past 3 s, after each has fired at least once since the last install (never-fired = UNKNOWN, not dead; bounded at 30 s so a failed install is still repaired), and a re-hook is DEFERRED for up to 10 s while a Space hold is live.
Two greps decide whether it took: `grep -c "WATCHDOG — " debug.log` should now be near 0 per hour (16 in 38 minutes was the 1.0.96 baseline), and `grep -c "would have alarmed" debug.log` shows what the OLD rule would have done — that line is deliberately kept so the data keeps accruing.
If the first count is still high, read the `EVIDENCED`/`UNEVIDENCED` words on those lines before changing anything: PROBLEM 228's section 9 (the OS input clock staying fresh while no hook is called) is still open and is the expected residual, not this rule failing.

**The trap that nearly re-opened PROBLEM 230, and it will catch you too:** the
old log printed a point-in-time witness clock and a 60–90 second *aggregate*
key count on different lines. Comparing them looks like "the primary sees keys
while the witness is silent" — the signature of this bug — when in fact the
keys all landed earlier in the window. **Two numbers may only be compared when
they describe the same window.** 1.0.96+ prints one line that fixes this:

```
grep "hook liveness split" debug.log | tail
```
`primary_real:R primary_injected:I reference:F mouse:M` — four callback-only
counters for ONE 60-second window. `primary_real` above 0 with `reference:0` is
this bug returning. Anything else is not.

## How to prove the fix took (two greps)

Run these against `%APPDATA%\Spaceadom\debug.log` after using the app normally
for a while (hold Space, tap letters, switch windows, use the dashboard):

```
grep "genuinely fired" debug.log | tail
```
The `(N total)` count in these lines must keep climbing over time. If it
freezes for minutes while the app is clearly being used, the witness is being
killed again and the order fix did not take (or something else now sits ahead
of it in the chain).

```
grep -c "WATCHDOG — " debug.log
```
This counts the false-alarm teardowns. It should stay near 0 over a normal
session. 514 in 3.8 hours was the broken baseline — that is the number a fix
has to beat.

## How to verify an AI's claim that it is fixed

Do not take "it's fixed" on its word. Three checks, in order:

1. **Marker-in-exe:** ask the AI to confirm a long, exact piece of the fixed
   code's log text (e.g. `"the REFERENCE keyboard hook failed to install"`) is
   present in the actual freshly-built `.exe`, not just in the source file —
   a fix in source that never got built and installed is not a fix.
2. **The differential rule:** this machine's agent shell runs inside a
   sandbox that silently redirects file paths, so reading a path from inside
   the shell can return a completely different, stale file while claiming to
   be the real one. The only valid proof is reading the *same* installed exe
   path from **both** the agent shell and a normal `explorer.exe`-launched
   process and comparing version number, byte size, and last-modified time.
   If they don't match, the "fix" was verified against the wrong file.
3. Only believe "fixed" once steps 1 and 2 both agree AND the two greps above
   show a climbing counter and a near-zero watchdog count on the log from the
   **real, installed** app — a clean build is not the same thing as a fixed
   machine.

## Teach-back (say this to an AI in under a minute)

Windows calls the newest keyboard hook first, and that hook's "am I too slow?"
timer includes everything that runs after it, all the way down the chain. We
had our health-check witness hook installed last, which put it first in line —
so it looked like the slowest thing in the whole chain, and Windows killed it,
silently, over and over. A dead witness can only ever report "all quiet," so
our own watchdog believed the real hook had gone deaf too, and kept ripping out
a hook that was actually working fine — 514 times in one afternoon. The fix was
one change: install the witness first, so it ends up last in line instead of
first. Never let anyone reorder `install_hooks()` in `hook/mod.rs` back to how
it was.

## 2026-09-06 FOLLOW-UP — hypothesis 3 came back TRUE on 1.0.101/1.0.102, and it is a different bug again (PROBLEM 257)

Hypothesis 3 above ("only while our own dashboard has focus") was disproved for
the PROBLEM 230 symptom and stays disproved for it. But on 2026-09-06 the owner's
hardware keys showed a NEW fault with exactly that shape: with the dashboard
focused, holding Space drew no ring and launched nothing; over every other app
it worked. The decisive line (00:54:52): our window foreground 60 of 60
samples, `mouse:2705 primary_real:0 reference:0` — the mouse hook on the same
thread firing while neither keyboard hook fired once. Not eviction (keys came
back the instant another window took focus, no re-hook), not a blocked pump,
not a gate (every gate counter 0). Keystrokes the OS accepted never reached any
keyboard hook in the process while our window had focus. Last good: 2026-09-05
14:11 (packaged 1.0.100). First bad: 15:35 (packaged 1.0.101).

What to do now: `grep "KEYBOARD DEAF, PROVEN" debug.log` (the watchdog proves
it with `GetAsyncKeyState(VK_SPACE)` — Space physically down, callback silent —
and re-hooks to the head of the chain), then read the next
`hook liveness split`. `grep "hold start"` — one line per hold the hook actually
saw, naming the window it was over; a hold with no line never reached the hook.
Law 6 in CLAUDE.md: no ship without a `hold start … over own window` followed by
`guide_hud: shown over own window` in the new build's log. A PREVIEW ("Check
the ring") prints the second line too and proves nothing — that is how 1.0.102's
log looked healthy.

## 2026-09-07 FOLLOW-UP — the watchdog was waiting for the MOUSE to stop moving (PROBLEM 260)

Read this before you look at any watchdog line in the log, and before you
conclude from a quiet log that the watchdog "did not notice". On 2026-09-07 an
independent `WH_KEYBOARD_LL` probe was run beside installed 1.0.103
(`_probe/ll-probe/events-run3.txt`). It bracketed 130 seconds of genuine
deafness in which **the watchdog printed nothing at all**.

### The mechanism, which nothing in this file had said before

* A `WH_KEYBOARD_LL` callback that overruns `LowLevelHooksTimeout`
  (`HKCU\Control Panel\Desktop`, 1000 ms by default) **stops being called and
  keeps a valid handle.** No message, no error, no return code says so, and
  `UnhookWindowsHookEx` on it still succeeds. From inside the process an
  evicted hook and a hook nobody has typed into are the same observation.
* **`WH_MOUSE_LL` is a separate hook with its own timeout record**, even on the
  same thread from the same pump. Evicting the keyboard hook does not touch it.
* Therefore the failure shape is **keyboard hooks dead, mouse hook alive** — and
  the mouse hook firing at 30–60 Hz makes the app look healthy from every clock
  except the keyboard's own.
* **The only cure a process has is to change the chain: unhook and install
  afresh.** The probe demonstrated this accidentally — a foreign keyboard hook
  installed by a *different process* at 10:12:34.551 had keys flowing to both it
  and Spaceadom 14 ms later.

### Why the watchdog said nothing for 130 seconds

It had two candidate tests and **neither can express that shape**:

* `both_dead` = keyboard silent **and mouse silent** past 3 s. The mouse hook
  was firing 3–4 times a second throughout (`mouse:217` in the 60 s window), so
  this was false on every tick.
* `kb_only_dead` = keyboard silent **and the reference hook still firing**. The
  reference was dead too (`reference:0`), so this was false on every tick.

The alarm that eventually fired at 10:12:34.246 did so **only because the mouse
happened to pause for 3032 ms at that instant**. The one line that says all of
it:

```
10:11:49  hook liveness split — primary_real:0 primary_injected:0 reference:0 mouse:217 in the last 60s
```

`mouse` above 0 with `primary_real` and `reference` both 0 is now a named
signature: **timeout eviction of the keyboard hooks with a healthy pump.** It is
not "nobody typed" and it is not a wedged thread.

### Two readings of the log that are WRONG, and will catch you too

**"`watchdog-reinstalls:1` twice in a row means the counter is stuck, so no
repair happened."** No. `HOOK_REINSTALLS` is **drained** every 60 s by
`drain_hook_diagnostics` (`swap(0)`), so that field is a *per-window count*.
Two windows reporting 1 means one reinstall in each. The proof a repair happened
is the alarm line itself: `Re-hooking. reinstall ok: true` is written **after**
`install_hooks()` returns.

**"`repair #1 this session` printed three times means that path never really
reinstalled."** The count was right; the word *session* was wrong — it read the
same drained counter. Fixed: the number now comes from `FORCED_REPAIRS_TOTAL`,
which nothing drains, and the line prints **old → new HHOOK values for all three
hooks**. A handle that did not change means the install failed. Never again
argue about whether a repair happened; read the handles.

### The rule now in force

**A PROVEN-deaf verdict must repair immediately, and no cooldown may block it.**
There is now a third candidate (`proven_keyboard_deaf` in `hook/mod.rs`) sitting
above every throttle. It fires when the keyboard callback has fired since the
install and then gone silent past the threshold **and** either Space is
physically down (PROBLEM 257's test) or the OS input clock is fresh while **our
own mouse callback cannot account for that input**. On that verdict the 60 s
cooldown and the "last repair delivered events" test are bypassed — they exist
to stop churn on **unevidenced** alarms (PROBLEM 236) and they keep that job in
full for the old path.

What still stops it, on purpose: the 10 s install grace, PROBLEM 228's
never-fired-is-UNKNOWN law, a latched Space hold (so it can never be "it dies
mid-press"), and a 5 s floor that doubles to a 60 s cap once repairs stop
delivering — with a line when that backoff engages.

### How to read the log after this

```
grep "KEYBOARD DEAF, PROVEN" debug.log      # the forced repairs, with handles
grep "hook liveness split" debug.log | tail # the next one says if it worked
```

`primary_real` above 0 in the split after a forced repair means the re-install
cured it. Still 0 means the drop is upstream of every hook in this process, and
re-hooking is not the answer — read the backoff line.

**Still unproven, so do not write it down as fact.** Whether the app's own
re-hook at 10:12:34.246 or the probe's install 305 ms later restored delivery
cannot be separated from the log. The `LowLevelHooksTimeout` mechanism above is
documented Windows behaviour and fits every number measured, but it is not
proven to be the cause of *this* episode. PROBLEM 260 fixes how fast the app
notices and repairs; it does not claim to fix the eviction.
