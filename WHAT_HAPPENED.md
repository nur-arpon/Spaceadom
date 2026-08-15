# What happened on 11 August 2026 — in plain English

Written by Claude Opus 5. The 10 August entry follows below; this is V14.

## The job

You handed over a working app (V13) and a finished design, and asked for the
design to be put onto the working app. You also asked me to throw away the
previous V14 and start again, keeping only the notes explaining what had gone
wrong.

Two earlier attempts had already tried this. Neither failed at writing code.
The first did the main window beautifully and never touched the pop-up
overlay. The second did the overlay beautifully and then rewrote the main
window from memory instead of copying the design file — and that rewrite was
rejected. Worse, when it rebuilt the folder it deleted the first attempt's
good main window without checking whether it was worth keeping. That work is
gone for good.

## So the first thing I did was not delete anything

I copied the old V14 somewhere safe first, then deleted it. It sits in
`D:\Claude-Projects\_V14-attempt2-archive` and is about 4 MB. You can delete
it once you're happy with the new build.

That turned out to matter. The old V14 contained real work that the notes did
not fully capture — an attempt at fixing two of your actual complaints (apps
flashing in the taskbar instead of coming to the front, and Store apps that
launch but never minimise). Its author labelled that code "written but never
verified".

**It was worse than unverified. It had never compiled.** When I brought it
across, the very first build failed on it. Two lines were asking Windows for
a feature that had never been switched on in the project's settings — the
previous session had written down which switches were needed in a notes file
and then not flipped them. Five-second fix, but only because the code still
existed to be fixed. Had I deleted the folder first, that work would have
vanished exactly the way the first attempt's did.

## Building the new one

The new V14 is your V13 — the same engine, the same keyboard hook, the same
everything that works — wearing the new design. It installs alongside V13 with
its own name and its own settings file, so the two cannot interfere with each
other or overwrite each other's shortcuts.

The overlay (the pop-up ring of shortcuts when you hold Space, and the little
notification pills) was copied across word for word from the attempt that got
it right. I did not touch its design. I did add one piece that had been
written down in the notes but never actually put into either version: the
window is now centred *before* it appears, so the ring doesn't jump from the
bottom of the screen to the middle on every show.

The main window I transcribed from your design file rather than interpreting
it — that being the exact mistake that sank the previous attempt. The old
sidebar, top bar and status bar are gone: it's now one warm cream stage with
the keyboard as the hero, profiles behind the pill top-right, settings behind
the gear bottom-left, and the special-key reference behind the pill at the
bottom. Clicking a key makes the editor bloom out of that key while the
keyboard blurs back behind it.

## Two things I checked rather than assumed

**The keyboard's width is 1048 pixels, not 1046.** Sixteen key-widths at the
design's own numbers comes to 1046, and that's what I wrote at first. But the
odd-sized keys — Shift, Caps, Enter — get rounded up to whole pixels one at a
time, and those roundings add two pixels to every row. I only know that
because I rendered it in a browser and measured it. Two pixels sounds
irrelevant; it's exactly enough to make the board spill out of its box at the
size where it only just fits, which is the size your screen will hit.

**The keyboard now has to fit twice over.** The previous attempt opened a
window wider than your monitor and the keyboard ran off the edge. There are
now two independent guards: the app sizes its own window to fit your screen,
and the keyboard shrinks itself to fit that window on both height and width —
not width alone, which is what broke before.

I checked the design faithfulness by measuring, not by looking: I rendered the
real components in a browser and read back every colour, size and radius the
finished page actually computes, then compared them to the design file. Key
height, corner radius, both key colours, the text colours, the glow sizes, the
background gradient — they match.

## What I have NOT done, and you need to know this

**I have not run it.** Not once.

Your V13 was running on this machine the whole time. Starting V14 next to it
would put two programs' hooks on your spacebar at the same time, which the
project notes specifically warn feeds back on itself and scrambles typing.
Stopping your running app wasn't my call to make.

So everything below is built and compiles cleanly, and is completely untested
in practice:

- the pop-up ring when you hold Space
- the notification pills
- dark mode reaching the overlay as well as the main window
- the key editor's open/close animation
- dragging an .exe onto a key
- the two fixes for apps flashing in the taskbar and Store apps not minimising

The overlay code is the version you personally approved, but "it worked in
that build" is not "it works in this one", and I'm not going to pretend
otherwise.

What it needs now is you: close V13, install V14, open it, hold Space, and
tell me what you actually see.

---

# What happened on 10 August 2026 — in plain English

> **Late-night update, same day:** the floating panel WORKS now. Hold Space
> and it appears over whatever you're doing — your actual shortcut keys, not
> a placeholder — and Space+F genuinely opens and toggles File Explorer. It
> took five separate buried problems stacked on top of each other, the
> deepest being a permissions file that quietly forbade the overlay window
> from ever receiving messages. Along the way we also broke and then fixed
> your touchpad gestures (sorry) — the app was accidentally minimizing parts
> of the Windows shell itself, which is why there is now a NATIVE_SAFETY.md
> file listing exactly what the app must never touch. Full story in
> PROJECT_STATUS.md, Part 3. Still open: the settings file saves twice, the
> UAC prompt on every launch is annoying, and the Boss Key / picture-in-
> picture features still need your hands to verify.

This is the readable version. No jargon where I can avoid it. If you want the
technical detail, it's in `PROJECT_STATUS.md` and `AI_HANDOFF.md`.

Written by Claude Opus 5.

---

## Where we started

The plan for the day was to look at your project and get something working.
Nothing built. Every attempt to compile the Rust code failed with the same
Windows error: "The system cannot find the file specified."

That error normally means Rust isn't installed. You said it was, and that this
was the same computer you'd built the earlier versions on. You were right, and
it's worth explaining what was actually going on, because it's a trap that
would fool almost anyone.

## The Rust problem

Rust was installed. The compiler was sitting there, all 194 MB of it. What had
gone wrong was smaller and much sneakier.

Rust keeps a set of small launcher files in a folder called `.cargo\bin`. When
you type `cargo`, Windows runs the launcher, and the launcher runs the real
compiler. Every one of those launcher files was **zero bytes**. Empty. And when
Windows tries to run an empty file, the error it gives you is "cannot find the
file specified" — which sends you off hunting for a missing file that is, in
fact, right there.

Worse, the usual way of checking ("does cargo.exe exist?") answers **yes**. The
file exists. It's just hollow.

I found the cause in the leftovers: a Rust update on 9 July at around 2:52 PM
was interrupted partway through. Rust empties those launcher files before
rewriting them, and it never got to the rewriting part. I checked whether your
antivirus had eaten them — it hadn't, Defender's history was clean apart from
some unrelated things.

Before reinstalling I searched all three of your drives, at full depth, for a
working copy. There wasn't one. I also found a Rust installer you'd downloaded
back in June, but its fingerprint didn't match the official one, so I didn't
run it.

The fix: Rust now lives in one clearly named folder, `D:\RUST-DOWNLOADED-HERE`,
instead of being scattered in hidden folders under your user account. Your
1.9 GB of downloaded packages was moved across rather than thrown away, so
nothing had to re-download. There's a plain-language README in that folder
explaining what's in it and how to fix it if it breaks again.

One thing to know: **the fresh install produced empty launcher files too.** So
this isn't bad luck, it's something your machine does reliably. The repair
takes about five seconds and the exact commands are in that README. Worth
remembering, because the standard advice you'll find online (`rustup default
stable`) does **not** fix it — Rust sees the empty files, assumes they're fine,
and skips them.

## The real discovery

With the build working, I read through the code looking for actual bugs. I
found ten. Most were ordinary. One explains the thing that's been frustrating
you for months.

You told me: v11 worked but you couldn't change the apps. July and Neon let you
change the apps but didn't really work. That's exactly right, and here's why.

**The overlay window was never created.**

The pop-up panel — the one that appears when you hold Space, and the little
notifications that say which app you just launched — needs its own window to
live in. A see-through one that floats on top of everything.

That window's design file was there. It was written correctly. The build system
was even packaging it up every time. But nothing ever opened it. Not one line
of code anywhere said "create this window."

So the app was doing its job perfectly: hold Space, and it sends off the
message "show the guide panel." The message just had nowhere to arrive. It got
delivered to the settings dashboard instead — which is minimised to the tray
during normal use. The panel was being drawn, faithfully, inside a hidden
window. Every time.

That's your hollow shell. The features were real. They were rendering somewhere
nobody could see.

There's a detail here I find telling. An earlier AI had added code that yanked
the settings window open every time you pressed a shortcut — so pressing
Space+B to open your browser threw the SpaceToggle window in your face first.
It looks like careless leftover code, and I nearly deleted it as such. It
wasn't. It was somebody's workaround: the only way to make an invisible
notification visible is to force open the window it's trapped in. They were
treating the symptom because the actual cause was invisible. At least two AIs
before me went digging through the panel's styling instead of checking whether
its window existed at all.

I built the window properly. Then I checked the running app to confirm Windows
really was treating it as see-through and click-through, rather than trusting
my own code. It reports the same window settings your v11 script asked
AutoHotkey for, which is a good sign — v11's overlay is the one you said worked.

One safety note, because it matters: this window covers your whole screen and
sits above everything. If the "let clicks pass through" setting ever failed,
you wouldn't be able to click on anything at all. The code now checks, and
hides the window if it can't confirm it. A missing panel is annoying; an
unclickable desktop is a disaster.

## The other nine

Briefly, in normal terms.

Fast typing could scramble letters. When you type quickly your finger is still
on Space as the next letter goes down. The old code sent the space and the
letter through two different routes, and the letter sometimes overtook the
space — so "the" came out as "hte". Both now travel together, in order.

The app ignored keystrokes from AutoHotkey, macro keyboards, on-screen
keyboards and Remote Desktop. It was trying to avoid reacting to its own
simulated keypresses, but the filter was far too broad and caught everyone
else's too. Some laptop keyboard drivers also mark genuinely physical keys this
way, which would make the app appear completely dead for no visible reason.

Ctrl+Space, Alt+Space and Windows+Space were being swallowed, which breaks
language switching, autocomplete in code editors, and the window menu.

The "how long to hold Space before the guide appears" slider in Settings did
nothing at all. It saved your number correctly. Nothing ever read it.

Every keypress leaked a small amount of memory. Tiny each time, but this runs
all day.

Every keypress also raised a Windows notification, so the Action Center filled
up with entries. That only existed because the in-app notification was
invisible.

Plus a couple of build warnings and a possible arithmetic overflow.

## Something you caught that I'd have missed

You mentioned partway through that v11 might still be running. It was —
`SpaceToggleRuntime.exe`, started the previous evening, launching automatically
from your Startup folder.

This matters more than it sounds. Two programs both hooking your spacebar will
fight. And because of the filter I'd just relaxed, they could get into a loop:
v11 sends a space, V13 sees it as a real keypress, sends its own, v11 sees that
one, and around it goes.

It also meant the testing I'd done up to that point was worthless — both
programs were running at once. I stopped v11 for the session only. Your Startup
shortcut is untouched, so v11 comes back when you reboot.

While testing, the app also quietly pointed your Windows startup entry at my
build folder. It does that every single time it launches, overwriting whatever
was there. I've removed it. That's arguably a bug in its own right — the app
shouldn't reassign your startup settings without asking — and it's on the list.

## Where I wasted time, and what I learned

I'm recording this because you asked for the mistakes too, and because these
cost real time.

I wrote a test program to type into a text box automatically and check what
came out. It reported everything as empty. My first thought was that the app
was swallowing every keystroke — which would have been alarming.

So I ran the same test with the app **switched off**. Still empty. The test
program was broken, not the app.

Two more rounds followed. Windows 11's Notepad runs as two processes and my
test was watching the wrong one, so it sat waiting for you to click a window
you'd probably already clicked. Then the real one: a size field in my code was
32 where Windows requires 40. Windows had been rejecting every simulated
keypress and returning an error code I never bothered to read. **Not a single
keystroke had been sent in any of the three runs.** Every "failure" I'd looked
at was my own bug.

Two lessons, and the first is the one I should have applied immediately:

Test the tool before trusting the test. Running the harness with the app turned
off would have caught this in the first minute instead of the fortieth.

And check what the system tells you. Windows was returning an error every
single time. I just never looked at it.

## What's genuinely confirmed, and what isn't

I want to be careful here, because your logs are full of earlier "it's fixed
now" claims that weren't.

Confirmed by actually running it: normal typing with spaces works. Tapping
Space gives exactly one space, no more. Ctrl+Space is left alone. Holding Space
down for over a second doesn't spray repeated spaces. Typing works normally
straight after using a shortcut. The overlay window exists and lets clicks
through.

Not confirmed, and I can't confirm it: whether holding Space and pressing a key
fires the shortcut, and whether the overlapped-typing fix works in practice.

There's a solid reason for that, not an excuse. Simulated keypresses don't
create the physical key state that the app checks for. The app has a safety
check asking "is Space genuinely held down right now?" — a real finger sets
that, a simulated keypress can't. So the shortcut can never fire under
automated testing, no matter how the test is written. It needs a human hand. I
proved this rather than assuming it, and wrote it down so nobody spends another
afternoon on it.

Still to fix: your settings file gets written to disk twice every time it
changes. Harmless but wrong, and it's been happening since at least July — I
can see the doubled entries in your old logs. And the startup entry behaviour.

## Where things stand

The build is clean — no errors, no warnings — and produces a working installer.
Rust is in one place with instructions. Ten bugs are fixed, including the one
that made two versions of this app feel like a shell.

What it needs now is you, holding Space and pressing F, and telling me what
actually happens.
