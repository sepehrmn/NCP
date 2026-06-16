# NCP TypeScript types (generated)

These `*.ts` files are the **canonical NCP message types for TypeScript**,
**generated from the Rust `ncp-core` types** via [ts-rs](https://github.com/Aleph-Alpha/ts-rs).
Rust is the single source of truth; TS, Python and Rust peers are therefore
wire-identical.

Do **not** edit by hand. Regenerate after changing the Rust types:

```bash
cargo test -p ncp-core --features ts     # rewrites this directory
```

Use them from a TS project (transport stays TS/Tauri/WebSocket — Zenoh is native,
so these are types only):

```ts
import type { SensorFrame, CommandFrame, ObservationFrame } from "./bindings";
```

Notes:
- enum values match the wire exactly (`Observable = "spikes" | "V_m" | "rate" | "weight"`);
- `NetworkRef.ref_` is emitted as `ref` (the wire name);
- Rust `i64` fields (`seq`, ids, `population_sizes`) are emitted as `bigint` by
  ts-rs for precision-safety; NCP uses small integers, so a consumer may treat
  them as `number` when parsing JSON.
