FROM target/consensource-rust:stable

COPY . /state_delta_subscriber
WORKDIR state_delta_subscriber
RUN cargo build

ENV PATH=$PATH:/state_delta_subscriber/target/debug/