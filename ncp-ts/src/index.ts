/**
 * Canonical NCP (Neuro-Cybernetic Protocol) for TypeScript: the generated,
 * wire-identical message types plus a transport-agnostic client.
 *
 * The types are generated (via ts-rs) from the `ncp-core` reference types, which
 * conform to the normative `proto/ncp.proto` wire contract (proto-native); the
 * client/transport add orchestration only. Rust, Python and TS peers are therefore
 * wire-identical. Do not re-declare these types downstream — import them from here.
 */

// Canonical, generated message types + enums (the JSON projection of proto/ncp.proto).
export type * from './generated'

// Client orchestration, JSON-wire helpers, and the WebSocket transport.
export {
  NeuroSimClient,
  NCP_VERSION,
  NCP_CONTRACT_HASH,
  checkVersion,
  NcpVersionError,
  contractStatus,
  assertScientificBoundary,
  NcpScientificBoundaryError,
} from './client'
export type {
  Send,
  Wire,
  ErrorFrame,
  ChannelInput,
  NetworkInput,
  RecordInput,
  StimulusInput,
  SimInput,
  SessionOpenedReply,
  SessionClosedReply,
  ObservationFrameReply,
  ObservationData,
} from './client'
export { WebSocketNeuroSim } from './ws'
