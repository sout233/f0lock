# FROZEN UI

Leaving my CLAP plugin open in FL Studio for about 3 minutes turned the spectrum analyzer into a flatline. Even worse: clicking the plugin after that would freeze FL Studio cold or send it straight to the desktop. 

Turns out, my code was being a bit *too* generous with Windows timers. It’s fully fixed now—here is what happened and how I squashed it.

---

## The Culprit: A Comedy of Two Timers

I accidentally introduced a classic Win32 timer leak into the redraw loop. 

To fetch a "unique" timer ID, my old implementation called `SetTimer(NULL, 0, ...)` to borrow a system-allocated ID, and then turned right around and called `SetTimer(hwnd, id, ...)` using that exact same number. 

Surprise! Windows didn't merge them—it cheerfully spun up **two entirely separate timers**:
1. Timer A: A stray, windowless thread timer.
2. Timer B: The actual window timer tied to my HWND.

When it came time to clean up, the code politely called `KillTimer(hwnd, id)`. Windows only killed Timer B, leaving Timer A running forever in the void, spamming phantom `WM_TIMER` messages into the host’s message pump. Because egui triggers redraw start/stop cycles like rapid-fire popcorn, orphaned timers piled up by the dozens until FL Studio choked on the spam.

```text
The messy cycle:
SetTimer(NULL, 0, ...)   → Spawns ghost Timer A (just to steal an ID)
SetTimer(hwnd, ID, ...)  → Spawns real Timer B
KillTimer(hwnd, ID)      → Kills B, but ghost Timer A lives on forever!

```

---

## The Fix: Taming the Timer Lifecycle

I threw out the "borrow an ID from Windows" nonsense and gave the plugin strict ownership over its timer IDs, making sure every timer that gets born actually gets cleaned up.

### 1. Stop summoning ghost timers

* **File:** `vendor/baseview/src/wrappers/win32/window/handle.rs`
* **What I did:**
* Swapped the workaround with an internal atomic counter (`AtomicUsize`) to mint clean, unique non-zero IDs locally.
* Axed `SetTimer(NULL, ...)`. Now it directly calls `SetTimer(hwnd, id, ...)` once and only once.
* Added clean error handling if the ID counter ever overflows—no hacky retries.



### 2. Clean up before the window dies

* **File:** `vendor/baseview/src/platform/win/window.rs`
* **What I did:**
* Hooked into `BaseviewWindow::before_destroy` to explicitly kill redraw, user, and delayed timers *while* the HWND is still valid and breathing.



### 3. Ditch the pointless post-mortem cleanup

* **File:** `vendor/baseview/src/platform/win/window_state.rs`
* **What I did:**
* Removed dead cleanup logic in `WindowSharedState::drop` that tried calling `KillTimer` on already-dead window handles (which was responsible for throwing ugly `Failed to kill timer: Invalid window handle` warnings).



---

## The New Clean Life Cycle

$$\text{Atomic Counter IDs} \longrightarrow \text{SetTimer}(hwnd, ID, \dots) \longrightarrow \text{KillTimer}(hwnd, ID) \longrightarrow \text{Destroy Window}$$

Every timer is now tied to a real window, destroyed cleanly, and leaves zero ghost messages behind. No more leaks, no more freezing, and FL Studio stays happy.


# How I FUCK FL Studio

If you kept my CLAP plugin window open in FL Studio for around 3 minutes during playback, the spectrum analyzer would flatline into a coma. Poking the host gave you maybe one stubborn frame back, and clicking anywhere on the plugin UI would instantly freeze FL or send it to the desktop.

Turns out, my redraw loop was enthusiastically leaking Win32 timers like an open fire hydrant. Here’s what broke and how I actually fixed it—no fluff, just the fix.

---

## The Root Cause: Accidental Ghost Timers

The bug came down to how I was asking Windows for timer IDs.

In my old code, `HWnd::create_timer` called `SetTimer(NULL, 0, ...)` just to grab a unique system-allocated ID. Then, it turned right around and called `SetTimer(hwnd, id, ...)` using that shiny new ID. 

What MSDN knows (and what I learned the hard way): **that doesn't bind the timer—it creates two entirely separate timers with the same number**:
1. **Timer A (Ghost):** A thread-level timer floating in the void with no HWND attached.
2. **Timer B (Real):** The window timer tied to my plugin's HWND.

When the frame ended, cleanup only ran `KillTimer(hwnd, id)`. That killed Timer B, but left Timer A ticking forever in the background. Because egui fires redraw requests constantly, every frame added another zombie timer to the pile, spamming `WM_TIMER` messages directly into FL Studio's UI thread until the message pump choked.

```text
Before the fix:
SetTimer(NULL, 0, ...)   → Spawns ghost Timer A just to borrow an ID
SetTimer(hwnd, ID, ...)  → Spawns window Timer B with the same ID
KillTimer(hwnd, ID)      → Kills B, but ghost Timer A lives forever!

```

---

## How I Fixed It

I completely cut out the "borrow an ID from Windows" dance, moved ID generation in-house, and cleaned up the teardown order.

### 1. Generate IDs locally instead of spawning OS timers

* **File:** `vendor/baseview/src/wrappers/win32/window/handle.rs`
* **Fix:**
* Replaced the `SetTimer(NULL, ...)` hack with a simple local `AtomicUsize` counter to hand out unique, non-zero IDs.
* Now we call `SetTimer(hwnd, id, ...)` directly. No ghost thread timers, zero spam.
* Added clean error handling if the ID counter ever wraps around—no weird retries or silent failures.



### 2. Kill timers while the window is still alive

* **File:** `vendor/baseview/src/platform/win/window.rs`
* **Fix:**
* Hooked into `BaseviewWindow::before_destroy` to explicitly stop all redraw, user, and delayed timers *before* the HWND gets invalidated by Windows.



### 3. Rip out dead post-mortem cleanup

* **File:** `vendor/baseview/src/platform/win/window_state.rs`
* **Fix:**
* Deleted redundant cleanup code in `WindowSharedState::drop` that was attempting to call `KillTimer` on an already-dead HWND. That killed off the annoying `Failed to kill timer: Invalid window handle` warnings for good.



---

## The Clean Lifecycle Now

$$\text{Atomic Counter (Local ID)} \longrightarrow \text{SetTimer}(hwnd, ID, \dots) \longrightarrow \text{KillTimer}(hwnd, ID) \longrightarrow \text{Destroy HWND}$$

Every timer created belongs to exactly one HWND, gets destroyed before the window dies, and zero orphaned timers hang around in the host process. FL Studio runs smooth, the spectrum analyzer doesn't die, and no more freezes.
