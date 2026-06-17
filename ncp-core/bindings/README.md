# NCP TypeScript types (generated)

These `*.ts` files are the **NCP message types for TypeScript**, currently
**generated from the Rust `ncp-core` types** via [ts-rs](https://github.com/Aleph-Alpha/ts-rs).
The normative wire contract is `proto/ncp.proto` (proto-native); `ncp-core` is its
reference implementation, so these types are wire-identical to the Rust, Python and
proto peers. (Migration target: generate directly from `proto/ncp.proto` via buf —
see `buf.gen.yaml`.)

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
