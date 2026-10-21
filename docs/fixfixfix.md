# FROZEN UI

Leaving my CLAP plugin open in FL Studio for about 3 minutes turned the spectrum analyzer into a flatline. Even worse: clicking the plugin after that would freeze FL Studio cold or send it straight to the desktop. 

Turns out, my code was being a bit *too* generous with Windows timers. It’s fully fixed now—here is what happened and how I squashed it.

---

## A Comedy of Two Timers

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

## Taming the Timer Lifecycle

I threw out the "borrow an ID from Windows" nonsense and gave the plugin strict ownership over its timer IDs, making sure every timer that gets born actually gets cleaned up.

### 1. Stop summoning ghost timers

* **File:** `vendor/baseview/src/wrappers/win32/window/handle.rs`
* Swapped the workaround with an internal atomic counter (`AtomicUsize`) to mint clean, unique non-zero IDs locally.
* Axed `SetTimer(NULL, ...)`. Now it directly calls `SetTimer(hwnd, id, ...)` once and only once.
* Added clean error handling if the ID counter ever overflows—no hacky retries.

### 2. Clean up before the window dies

* **File:** `vendor/baseview/src/platform/win/window.rs`
* Hooked into `BaseviewWindow::before_destroy` to explicitly kill redraw, user, and delayed timers *while* the HWND is still valid and breathing.

### 3. Ditch the pointless post-mortem cleanup

* **File:** `vendor/baseview/src/platform/win/window_state.rs`
* Removed dead cleanup logic in `WindowSharedState::drop` that tried calling `KillTimer` on already-dead window handles (which was responsible for throwing ugly `Failed to kill timer: Invalid window handle` warnings).

---

## The New Clean Life Cycle

$$\text{Atomic Counter IDs} \longrightarrow \text{SetTimer}(hwnd, ID, \dots) \longrightarrow \text{KillTimer}(hwnd, ID) \longrightarrow \text{Destroy Window}$$

Every timer is now tied to a real window, destroyed cleanly, and leaves zero ghost messages behind. No more leaks, no more freezing, and FL Studio stays happy.
