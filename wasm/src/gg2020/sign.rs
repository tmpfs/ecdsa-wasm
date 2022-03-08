use curv::{arithmetic::Converter, elliptic::curves::Secp256k1, BigInt};
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
    party_i::{verify, SignatureRecid},
    state_machine::{
        keygen::LocalKey,
        sign::{
            CompletedOfflineStage, OfflineProtocolMessage, OfflineStage,
            PartialSignature, SignManual,
        },
    },
};

use round_based::{Msg, StateMachine};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

//use crate::{console_log, log};

const ERR_COMPLETED_OFFLINE_STAGE: &str =
    "completed offline stage unavailable, has partial() been called?";

/// Wrapper for a round `Msg` that includes the round
/// number so that we can ensure round messages are grouped
/// together and out of order messages can thus be handled correctly.
#[derive(Serialize)]
struct RoundMsg {
    round: u16,
    sender: u16,
    receiver: Option<u16>,
    body: OfflineProtocolMessage,
}

impl RoundMsg {
    fn from_round(
        round: u16,
        messages: Vec<Msg<<OfflineStage as StateMachine>::MessageBody>>,
    ) -> Vec<Self> {
        messages
            .into_iter()
            .map(|m| RoundMsg {
                round,
                sender: m.sender,
                receiver: m.receiver,
                body: m.body,
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignResult {
    signature: SignatureRecid,
    #[serde(rename = "publicKey")]
    public_key: Vec<u8>,
    address: String,
}

#[wasm_bindgen]
pub struct Signer {
    inner: OfflineStage,
    completed: Option<(CompletedOfflineStage, BigInt)>,
}

#[wasm_bindgen]
impl Signer {
    #[wasm_bindgen(constructor)]
    pub fn new(
        index: JsValue,
        participants: JsValue,
        local_key: JsValue,
    ) -> Result<Signer, JsError> {
        let index: u16 = index.into_serde()?;
        let participants: Vec<u16> = participants.into_serde()?;
        let local_key: LocalKey<Secp256k1> = local_key.into_serde()?;
        Ok(Signer {
            inner: OfflineStage::new(index, participants.clone(), local_key)?,
            completed: None,
        })
    }

    #[wasm_bindgen(js_name = "handleIncoming")]
    pub fn handle_incoming(&mut self, message: JsValue) -> Result<(), JsError> {
        let message: Msg<<OfflineStage as StateMachine>::MessageBody> =
            message.into_serde()?;
        self.inner.handle_incoming(message)?;
        Ok(())
    }

    pub fn proceed(&mut self) -> Result<JsValue, JsError> {
        if self.inner.wants_to_proceed() {
            self.inner.proceed()?;
            let messages = self.inner.message_queue().drain(..).collect();
            let round = self.inner.current_round();
            let messages = RoundMsg::from_round(round, messages);
            Ok(JsValue::from_serde(&(round, &messages))?)
        } else {
            Ok(JsValue::from_serde(&false)?)
        }
    }

    pub fn partial(&mut self, message: JsValue) -> Result<JsValue, JsError> {
        let message: String = message.into_serde()?;
        let completed_offline_stage = self.inner.pick_output().unwrap()?;
        let data = BigInt::from_bytes(message.as_bytes());
        let (_sign, partial) =
            SignManual::new(data.clone(), completed_offline_stage.clone())?;

        self.completed = Some((completed_offline_stage, data));

        Ok(JsValue::from_serde(&partial)?)
    }

    pub fn create(&mut self, partials: JsValue) -> Result<JsValue, JsError> {
        let partials: Vec<PartialSignature> = partials.into_serde()?;

        let (completed_offline_stage, data) = self
            .completed
            .take()
            .ok_or_else(|| JsError::new(ERR_COMPLETED_OFFLINE_STAGE))?;
        let pk = completed_offline_stage.public_key().clone();

        let (sign, _partial) =
            SignManual::new(data.clone(), completed_offline_stage.clone())?;

        let signature = sign.complete(&partials)?;
        verify(&signature, &pk, &data).map_err(|e| {
            JsError::new(&format!("failed to verify signature: {:?}", e))
        })?;

        let public_key = pk.to_bytes(false).to_vec();
        let result = SignResult {
            signature,
            address: crate::utils::address(&public_key),
            public_key,
        };

        Ok(JsValue::from_serde(&result)?)
    }
}
