# NCP for neuromorphic hardware — and for sim-before-deploy

NCP's backend is a `SimulationBackend` trait (`backends.py`): NEST today, but the
**wire contract knows nothing about NEST**. It speaks neural record/stimulus
(`V_m`/`spikes`/`rate`, `current_pA`/`rate_hz`/`spike_times`) against named
populations, advanced in `Run`-style chunks. That is exactly the surface a
**neuromorphic chip** also exposes — so NCP is most useful as the **stable
interface across the simulation → hardware transition**.

## Where NCP helps

### 1. One interface, swap the substrate (the headline use)
A robot/UAV client and an analysis/observer client are written against
NCP, not against NEST. Replace the backend — NEST → a neuromorphic chip — behind
the *same* `OpenSession`/`StepRequest`/`ObservationFrame` wire, and **no client
changes**. The brain's substrate becomes a deployment choice, not an API. This is
the single biggest reason to put NCP between the SNN and everything else *before*
you ever touch silicon.

Concrete backend adapters (each a `SimulationBackend`, mirroring `NestBackend`'s
`Prepare`/`Run(chunk)`/`Cleanup` + record/stimulus mapping):
- **Intel Loihi 2 via Lava** — Lava `Process` graphs with `run(RunSteps/RunContinuous)`
  map onto NCP `open`/`step`/`run`; inject via Lava input ports (≈ NCP stimulus),
  read via output ports / probes (≈ NCP record). Real-time / faster-than-real-time.
- **SpiNNaker / SpiNNaker 2 via sPyNNaker (PyNN)** — PyNN `run(t)` is the chunked
  advance; `pop.record(...)` and `pop.set(...)` are the record/stimulus surface;
  SpiNNaker's real-time operation suits the control loop.
- **BrainScaleS-2** — analog, ~10³–10⁴× accelerated: excellent for fast design
  sweeps, but its I/O cadence differs; NCP's `chunk_ms` and the resilience layer
  still frame the exchange.
- **Akida / other event-based accelerators** — same record/stimulus mapping where
  a spiking I/O API exists.

These are *adapters behind the existing trait*; they need **no protocol change**.

### 2. Sim-before-deploy workflow
The standard neuromorphic path is develop+validate in simulation, then deploy to
chip. NCP makes each stage the same wire:
1. **Develop** the closed loop (sensor → SNN → command) against the **NEST**
   backend over NCP — fast iteration, full observability, the `NeuroControlLoop`.
2. **Validate** with the streaming control plane + the resilience layer
   (`ActionBuffer`, `LinkMonitor`) already in place — the robot link behaves the
   same whether the brain is sim or chip.
3. **Deploy** by switching the backend to the chip adapter. The client, the codec,
   the safety governor, and the analysis tap are unchanged.

### 3. Differential (sim-vs-hardware) testing — for free
Because NCP records observations against a declared surface, you can replay the
**same stimulus trace** into the NEST backend and the chip backend and diff the
`ObservationFrame`s. That is a clean A/B for **sim-to-hardware fidelity** (does the
chip's spiking match the simulator within tolerance?), using nothing but the
record/stimulus contract. The scientific boundary makes this honest: every frame
is `is_simulation_output=true` / `calibrated_posterior=false`, so neither sim nor
chip output is mistaken for a validated claim — and on analog hardware
(BrainScaleS device mismatch, trial-to-trial variability) that disclaimer matters
even more than in sim.

### 4. Hardware-in-the-loop (HIL)
A chip becomes an NCP backend; the robot or a physics sim is the NCP client. The
resilience layer (`RESILIENCE.md`) is now load-bearing: chip↔host and host↔robot
are real, lossy links, and the `ActionBuffer`/`ttl_ms`/HOLD fail-safe + the
`LinkMonitor` apply unchanged. NCP turns "SNN on a chip driving a real robot" into
the same protocol you debugged in sim.

### 5. An information-theoretic analysis client as a sim-to-hardware fidelity metric
An analysis/observer client (e.g. one computing Partial Information Decomposition /
PID) can tap NCP over the `(V,L,D,A)` observation stream. Run it against the NEST
backend *and* the chip backend and you
get an **information-theoretic** comparison: does the chip preserve the same
unique/redundant/synergistic information flow {sensors → action} as the simulator,
or does device noise/quantization destroy a synergistic channel? That is a far
sharper "did porting to hardware change the computation?" test than trace RMSE.

## Honest limits
- These backend adapters are **not yet implemented** — `NestBackend` is the only
  live one. The point here is that NCP's contract *admits* them with no wire
  change; building a `LavaBackend` / `SpiNNakerBackend` is scoped follow-up work.
- Chips constrain models (fixed neuron types, fan-in, weight precision); a model
  that runs in NEST may need adaptation for silicon. NCP doesn't hide that — it
  makes the *interface* constant, not the model.
- Hard real-time guarantees come from the chip + its host stack (SpiNNaker is
  strong here), not from NCP; NCP provides the chunked exchange, QoS, and
  fail-safe around it.
