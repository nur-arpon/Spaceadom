# Vanilla TypeScript Patterns

For a Tauri project with no frontend framework — plain HTML, CSS, and TypeScript modules with Vite.

This is a legitimate choice for a desktop utility. It starts faster, uses less memory, and has no reconciler between an event and the DOM. What it costs is structure: nothing enforces the separation between state and view, so it degrades into `querySelector` calls scattered across files unless you impose a pattern.

**Do not add React to fix a problem in this list.** Every item has a working vanilla answer.

## State without a framework

Use `nanostores` (1.4.2, ~1 KB) rather than hand-rolling an event emitter.

```ts
// src/state.ts
import { atom, map, computed } from 'nanostores';

export const $activeProfile = atom<string>('default');
export const $profiles = map<Record<string, Profile>>({});
export const $bypassed = atom(false);

export const $currentBindings = computed(
  [$activeProfile, $profiles],
  (id, profiles) => profiles[id]?.bindings ?? {}
);
```

```ts
// src/ui/profile-list.ts
import { $profiles, $activeProfile } from '../state';

const list = document.getElementById('profile-list')!;

$profiles.subscribe(render);
$activeProfile.subscribe(render);

function render() {
  const profiles = $profiles.get();
  const active = $activeProfile.get();
  list.replaceChildren(
    ...Object.values(profiles).map(p => profileRow(p, p.id === active))
  );
}
```

The rule that keeps this maintainable: **state changes go through the store, never directly to the DOM.** A click handler calls `$activeProfile.set(id)` and returns. Rendering is a subscriber. Once handlers start mutating the DOM directly, state and view drift and you get the class of bug where the UI shows something the data does not.

## Rendering lists

`replaceChildren` with fresh nodes is fast enough for a few hundred rows and far simpler than diffing. Above that, keep nodes and update in place, or virtualize with `@tanstack/virtual-core`.

For templates, `<template>` beats string concatenation — it parses once and clones cheaply, and it cannot produce an XSS hole through interpolation:

```html
<template id="tpl-profile-row">
  <div class="profile-row" role="listitem">
    <span class="profile-name"></span>
    <span class="profile-count"></span>
  </div>
</template>
```

```ts
const tpl = document.getElementById('tpl-profile-row') as HTMLTemplateElement;

function profileRow(p: Profile, active: boolean): HTMLElement {
  const node = tpl.content.firstElementChild!.cloneNode(true) as HTMLElement;
  node.querySelector('.profile-name')!.textContent = p.name;      // safe by construction
  node.querySelector('.profile-count')!.textContent = `${p.count}`;
  node.classList.toggle('is-active', active);
  node.dataset.id = p.id;
  return node;
}
```

Never build DOM with `innerHTML` and template strings containing user data. In a desktop app the "user data" includes application names and file paths read off the system, which are not trustworthy.

## Event delegation

Do not attach a listener per row. One listener on the container, dispatch by `data-*`:

```ts
list.addEventListener('click', (e) => {
  const row = (e.target as HTMLElement).closest<HTMLElement>('[data-id]');
  if (!row) return;
  $activeProfile.set(row.dataset.id!);
});
```

This survives re-rendering with no rebinding and no leaked listeners — the two most common memory problems in hand-written frontends.

## Components as custom elements

When a piece of UI has its own lifecycle, a custom element gives structure without a framework:

```ts
class KeyBinder extends HTMLElement {
  #abort = new AbortController();

  connectedCallback() {
    this.tabIndex = 0;
    this.addEventListener('keydown', this.#onKey, { signal: this.#abort.signal });
  }

  disconnectedCallback() {
    this.#abort.abort();          // every listener removed at once
  }

  #onKey = (e: KeyboardEvent) => {
    e.preventDefault();
    this.dispatchEvent(new CustomEvent('bind', { detail: e.code, bubbles: true }));
  };
}
customElements.define('key-binder', KeyBinder);
```

The `AbortController` pattern is the cleanest teardown available — one `abort()` removes every listener registered with that signal, so nothing leaks when the element is removed.

Custom elements also compose with `@fluentui/web-components`, which is itself a set of custom elements. Mixing your own with Microsoft's works naturally.

## Fluent components without a framework

```bash
npm i @fluentui/web-components @fluentui/tokens
```

```ts
import { setTheme } from '@fluentui/web-components';
import { webDarkTheme, webLightTheme } from '@fluentui/tokens';
import '@fluentui/web-components/button.js';
import '@fluentui/web-components/switch.js';
import '@fluentui/web-components/text-input.js';

setTheme(matchMedia('(prefers-color-scheme: dark)').matches ? webDarkTheme : webLightTheme);
```

Import per-component so unused controls stay out of the bundle. This is the highest-leverage import for a vanilla Windows app — native Fluent appearance and behavior with no controls written by hand.

## Animation without a framework

Everything in `motion-physics.md` applies. Both major libraries work here:

```ts
import gsap from 'gsap';
gsap.to('.hud', { duration: 0.2, opacity: 1, scale: 1, ease: 'power2.out' });
```

```ts
import { animate } from 'motion';   // vanilla API, not motion/react
animate('.hud', { opacity: 1, scale: 1 }, { type: 'spring', stiffness: 300, damping: 30 });
```

```ts
import autoAnimate from '@formkit/auto-animate';
autoAnimate(document.getElementById('profile-list')!);
```

AutoAnimate is the one to reach for first — it handles the add/remove/reorder case that is otherwise fiddly, in one line, for 3.2 KB.

For enter/exit without any library, `@starting-style` plus `transition-behavior: allow-discrete` covers it. See `motion-physics.md`.

## Typed IPC

Without a framework there is no compile-time link between Rust commands and TypeScript calls. Two options, in order of preference:

**Generate the types** with `tauri-specta` — commands, arguments, and returns become TypeScript automatically, so a Rust rename breaks the build.

**Or wrap and validate** at the boundary:

```ts
// src/ipc.ts
import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';

const Profile = z.object({
  id: z.string(),
  name: z.string(),
  bindings: z.record(z.string(), z.string()),
});
export type Profile = z.infer<typeof Profile>;

export async function loadProfiles(): Promise<Profile[]> {
  return z.array(Profile).parse(await invoke('load_profiles'));
}
```

One module owns every `invoke` call. Nothing else in the app imports from `@tauri-apps/api/core`. This makes the full IPC surface greppable and gives one place to add logging or error handling.

## Event listeners from Rust

```ts
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

const unlisteners: UnlistenFn[] = [];

export async function startListening() {
  unlisteners.push(await listen<string>('profile-changed', (e) => {
    $activeProfile.set(e.payload);
  }));
}

export function stopListening() {
  unlisteners.forEach(fn => fn());
  unlisteners.length = 0;
}
```

Discarding the unlisten function leaks the handler and everything it captures. Collect them.

## Multi-window

With `rollupOptions.input` mapping several HTML entry points, each window is a separate document with a separate JS context. They share nothing — not modules, not state.

Communicate through Rust events, not `postMessage` or `localStorage`:

```ts
// overlay window
import { listen } from '@tauri-apps/api/event';
listen<Bindings>('hud-show', (e) => renderHud(e.payload));
```

```rust
app.emit_to("overlay", "hud-show", bindings)?;
```

Keep each window's entry bundle minimal — an overlay that imports the settings UI's dependencies pays for them in both startup time and per-renderer memory.

## CSS organization

Without CSS-in-JS or utility classes, one convention has to be chosen and held. Layers plus custom properties work well:

```css
@layer reset, tokens, base, components, utilities;

@layer tokens {
  :root {
    --space-1: 4px;  --space-2: 8px;  --space-4: 16px;
    --radius-control: 4px;  --radius-surface: 8px;
    --ease-out: cubic-bezier(0.2, 0.8, 0.2, 1);
    --dur-fast: 120ms;  --dur-base: 200ms;
  }
}

@layer components {
  .btn {
    height: 32px;
    padding-inline: var(--space-4);
    border-radius: var(--radius-control);
    transition: background var(--dur-fast) var(--ease-out);
  }
}
```

`@layer` removes specificity fights permanently — a later layer always wins regardless of selector weight, so `!important` stops being necessary.

Import `open-props` for the token layer rather than authoring one. See `libraries-frontend.md`.

## Project structure

```
src/
├── main.ts              # entry: theme, listeners, mount
├── state.ts             # nanostores
├── ipc.ts               # every invoke() lives here
├── styles/
│   ├── tokens.css
│   ├── base.css
│   └── components.css
├── ui/
│   ├── profile-list.ts
│   ├── key-binder.ts
│   └── toast.ts
└── overlay/
    ├── main.ts          # separate entry for overlay.html
    └── hud.ts
```

The rule worth enforcing: **`ui/` modules never import from each other.** They read state and emit events. Cross-imports between UI modules are how a hand-written frontend becomes untraceable.

## When a framework is actually warranted

Be honest about this rather than reflexively defending either choice. Signals that vanilla has stopped paying:

- More than roughly 15 interactive views
- Deeply nested state where one change must update many distant places
- Repeated manual sync bugs between store and DOM
- Complex form validation across many fields
- A team where people cannot keep the conventions in their heads

None of these apply to a settings window plus an overlay. For that shape, vanilla is the better engineering choice, and migrating would be a rewrite that buys nothing.
