FROM target/consensource:rust-base-1.38

COPY . /state_delta_subscriber
WORKDIR state_delta_subscriber
RUN cargo build

ENV PATH=$PATH:/state_delta_subscriber/target/debug/