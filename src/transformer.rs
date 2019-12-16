use protobuf::Message;

/// A container of messages
///
/// This is a generic container of protobuf messages.  It is a trivial wrapper
/// for any Protobuf message that simply defines a list of other Protobuf
/// messages.
pub trait Container<S>
where
    S: Message,
{
    /// Returns the values stored in this container
    fn values(&self) -> &[S];

    /// Transforms this Container of Protobuf messages to a type that implements
    /// FromStateAtBlock for that message.
    fn to_models<D>(&self, at_block_num: i64) -> Vec<D>
    where
        D: FromStateAtBlock<S>,
    {
        self.values()
            .iter()
            .map(|state_value| FromStateAtBlock::at_block(at_block_num, state_value))
            .collect()
    }
}

#[macro_export]
macro_rules! containerize {
    ($val_type:path, $container_type:path) => {
        containerize!($val_type, $container_type, entries);
    };
    ($val_type:path, $container_type:path, $container_field:ident) => {
        impl ::transformer::Container<$val_type> for $container_type {
            fn values(&self) -> &[$val_type] {
                &self.$container_field
            }
        }
    };
}

/// A trait for transforming a Protobuf message into an object at a particular
/// block height.
pub trait FromStateAtBlock<S>
where
    S: Message,
{
    /// Returns an instance of Self in relation to the current block number
    /// with the values described in the given state value.
    fn at_block(block_num: i64, state_value: &S) -> Self;
}
