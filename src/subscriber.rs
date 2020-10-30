use common::addressing::get_family_namespace_prefix;
use errors::SubscriberError;
use event_handler::EventHandler;
use protobuf;
use sawtooth_sdk::messages::client_event::{
    ClientEventsSubscribeRequest, ClientEventsSubscribeResponse,
    ClientEventsSubscribeResponse_Status, ClientEventsUnsubscribeRequest,
    ClientEventsUnsubscribeResponse, ClientEventsUnsubscribeResponse_Status,
};
use sawtooth_sdk::messages::events::{EventFilter, EventFilter_FilterType, EventSubscription};
use sawtooth_sdk::messages::validator::Message_MessageType;
use sawtooth_sdk::messaging::stream::{MessageConnection, MessageReceiver, MessageSender};
use sawtooth_sdk::messaging::zmq_stream::{ZmqMessageConnection, ZmqMessageSender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const NULL_BLOCK_ID: &str = "0000000000000000";
const KNOWN_COUNT: usize = 10;

/// Subscribes to the validator for block-commit and state-delta events
/// Listens to events and calls the event handler to parse event and submit the data to the reporting database
pub struct Subscriber {
    sender: ZmqMessageSender,
    receiver: MessageReceiver,
    event_handler: EventHandler,
    pub active: Arc<AtomicBool>,
}

impl Subscriber {
    pub fn new(validator_address: &str, event_handler: EventHandler) -> Subscriber {
        let zmq = ZmqMessageConnection::new(validator_address);
        let (sender, receiver) = zmq.create();
        Subscriber {
            sender,
            receiver,
            event_handler,
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Sends a subscription request to the validator, with a list of known block ids
    /// If the request is successful, it start listening for block-commit and state-delta events
    /// ```
    /// # Errors
    /// It returns an error if
    /// - It fails to connect to the validator
    /// - The validator responds with an error
    /// - The event handler returns an error
    ///
    /// # Panic
    /// It panics if
    /// - If it fails to serialize the event subscription request to bytes.
    /// - It fails to deserialize the validator response to a protobuf message
    /// ```
    pub fn start(
        &mut self,
        known_block_ids: &[String],
        start_index: usize,
    ) -> Result<(), SubscriberError> {
        let last_known_block_ids = self.get_last_known_block_ids(known_block_ids, start_index);
        let event_subscription_request = self.build_subscription_request(&last_known_block_ids);
        let content = protobuf::Message::write_to_bytes(&event_subscription_request)
            .expect("Error writing to bytes");
        let correlation_id = Uuid::new_v4().to_string();
        let mut response_future = self
            .sender
            .send(
                Message_MessageType::CLIENT_EVENTS_SUBSCRIBE_REQUEST,
                &correlation_id,
                &content,
            )
            .map_err(|err| SubscriberError::ConnError(err.to_string()))?;
        let future_result = response_future
            .get()
            .map_err(|err| SubscriberError::ConnError(err.to_string()))?;
        let response: ClientEventsSubscribeResponse =
            protobuf::parse_from_bytes(&future_result.get_content())
                .expect("Error parsing protobuf data.");
        match response.get_status() {
            ClientEventsSubscribeResponse_Status::OK => {
                info!("Successfully subscribed to receive events from validator");
                self.active.swap(true, Ordering::SeqCst);

                while self.active.load(Ordering::SeqCst) {
                    let messaged_received = self.receiver.recv_timeout(Duration::from_millis(1000));
                    if messaged_received.is_ok() {
                        let received = messaged_received.unwrap().expect("Unexpected error");
                        self.event_handler.handle_events(received.get_content())?;
                    }
                }
                self.stop()?;
                Ok(())
            }
            ClientEventsSubscribeResponse_Status::UNKNOWN_BLOCK => {
                debug!("Validator returned UNKNOWN_BLOCK response. Trying again with new set of blocks");
                self.start(known_block_ids, start_index + KNOWN_COUNT)
            }
            _ => Err(SubscriberError::ConnError(format!(
                "The valiator returned an invalid response {:?}",
                response.get_status()
            ))),
        }
    }

    /// Sends a unsubscribe request to the validator,
    /// ```
    /// # Errors
    /// It returns an error if
    /// - It fails to connect to the validator
    /// - The validator responds with an error
    /// - The event handler returns an error
    ///
    /// # Panic
    /// It panics if
    /// - If it fails to serialize the event subscription request to bytes.
    /// - It fails to deserialize the validator response to a protobuf message
    /// ```
    pub fn stop(&mut self) -> Result<(), SubscriberError> {
        let unsusbscribe_request = ClientEventsUnsubscribeRequest::new();
        let content = protobuf::Message::write_to_bytes(&unsusbscribe_request)
            .expect("Error writting protobuf data.");
        let correlation_id = Uuid::new_v4().to_string();
        let mut response_future = self
            .sender
            .send(
                Message_MessageType::CLIENT_EVENTS_UNSUBSCRIBE_REQUEST,
                &correlation_id,
                &content,
            )
            .map_err(|err| SubscriberError::ConnError(err.to_string()))?;
        let future_result = response_future
            .get()
            .map_err(|err| SubscriberError::ConnError(err.to_string()))?;
        let response: ClientEventsUnsubscribeResponse =
            protobuf::parse_from_bytes(&future_result.get_content())
                .expect("Error parsing protobuf data.");
        match response.get_status() {
            ClientEventsUnsubscribeResponse_Status::OK => {
                info!("Successfully unsubscribed from receiving events from validator");
                self.sender.close();
                Ok(())
            }
            _ => Err(SubscriberError::ConnError(format!(
                "The valiator returned an invalid response {:?}",
                response.get_status()
            ))),
        }
    }

    /// Given a list of known block ids, returns a list of at most 10 last know block ids starting
    /// from start_index.
    /// If start_index is greaten than the input list of known block ids, it returns
    /// an array with the NULL_BLOCK_ID. The NULL_BLOCK_ID is the standard ID for the genesis block
    fn get_last_known_block_ids(
        &self,
        known_block_ids: &[String],
        start_index: usize,
    ) -> Vec<String> {
        if start_index >= known_block_ids.len() {
            debug!("Subscribing to events starting from the genesis block");
            vec![NULL_BLOCK_ID.to_string()]
        } else if (start_index + KNOWN_COUNT) >= known_block_ids.len() {
            debug!(
                "Subscribing to events with known block ids {:?}",
                known_block_ids[start_index..].to_vec()
            );
            known_block_ids[start_index..].to_vec()
        } else {
            debug!(
                "Subscribing to events with known block ids {:?}",
                known_block_ids[start_index..start_index + KNOWN_COUNT].to_vec()
            );
            known_block_ids[start_index..start_index + KNOWN_COUNT].to_vec()
        }
    }

    fn build_subscription_request(
        &self,
        last_known_block_ids: &[String],
    ) -> ClientEventsSubscribeRequest {
        let block_subscription = self.get_block_commit_subscription();
        let state_delta_subscription = self.get_state_delta_subscription();

        let mut event_subscription_request = ClientEventsSubscribeRequest::new();
        event_subscription_request.set_subscriptions(protobuf::RepeatedField::from_vec(vec![
            block_subscription,
            state_delta_subscription,
        ]));
        event_subscription_request.set_last_known_block_ids(protobuf::RepeatedField::from_vec(
            last_known_block_ids.to_vec(),
        ));

        event_subscription_request
    }

    fn get_block_commit_subscription(&self) -> EventSubscription {
        let mut block_commit_subscription = EventSubscription::new();
        block_commit_subscription.set_event_type(String::from("sawtooth/block-commit"));
        block_commit_subscription
    }

    fn get_state_delta_subscription(&self) -> EventSubscription {
        let mut state_delta_subscription = EventSubscription::new();
        state_delta_subscription.set_event_type(String::from("sawtooth/state-delta"));

        let mut event_filter = EventFilter::new();
        event_filter.set_key(String::from("address"));

        let namespace = get_family_namespace_prefix();
        event_filter.set_match_string(format!(r"^{}", namespace));

        let event_filter_type = EventFilter_FilterType::REGEX_ANY;
        event_filter.set_filter_type(event_filter_type);

        let event_filter_vec = vec![event_filter];
        let repeated_field = protobuf::RepeatedField::from_vec(event_filter_vec);

        state_delta_subscription.set_filters(repeated_field);
        state_delta_subscription
    }
}
