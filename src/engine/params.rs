//! Engine Parameters.

use std::time::Duration;

/// The default engine api authentication port.
pub const DEFAULT_AUTH_PORT: u16 = 8551;

/// The ID of the static payload
pub const STATIC_ID: u32 = 1;

/// The json rpc version string
pub const JSONRPC_VERSION: &str = "2.0";

/// The new payload method string
pub const ENGINE_NEW_PAYLOAD_V2: &str = "engine_newPayloadV2";

/// The new payload timeout
pub const ENGINE_NEW_PAYLOAD_TIMEOUT: Duration = Duration::from_secs(8);

/// The get payload method string
pub const ENGINE_GET_PAYLOAD_V2: &str = "engine_getPayloadV2";

/// The get payload timeout
pub const ENGINE_GET_PAYLOAD_TIMEOUT: Duration = Duration::from_secs(2);

/// The forkchoice updated method string
pub const ENGINE_FORKCHOICE_UPDATED_V2: &str = "engine_forkchoiceUpdatedV2";

/// The forkchoice updated timeout
pub const ENGINE_FORKCHOICE_UPDATED_TIMEOUT: Duration = Duration::from_secs(8);
