use serde::{Serialize, Serializer};
use std::marker::PhantomData;

macro_rules! address {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[repr(transparent)]
        pub struct $name<T> {
            address: u64,
            _pointee: PhantomData<fn() -> T>,
        }

        impl<T> $name<T> {
            /// Construct backend-provided GPU address metadata.
            ///
            /// This does not validate resource liveness, bounds, layout, or access.
            /// The backend must uphold the address type's shader access contract.
            pub fn from_raw(address: u64) -> Self {
                Self {
                    address,
                    _pointee: PhantomData,
                }
            }

            pub fn to_raw(self) -> u64 {
                self.address
            }
        }

        impl<T> Clone for $name<T> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<T> Copy for $name<T> {}

        impl<T> std::fmt::Debug for $name<T> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    f,
                    "{}<{}>({:#x})",
                    stringify!($name),
                    std::any::type_name::<T>(),
                    self.address
                )
            }
        }

        impl<T> Serialize for $name<T> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u64(self.address)
            }
        }

        const _: () = assert!(std::mem::size_of::<$name<()>>() == 8);
        const _: () = assert!(std::mem::align_of::<$name<()>>() == 8);
    };
}

address!(
    Addr,
    "A GPU buffer device address pointing at std430-laid-out elements."
);
address!(
    ReadAddr,
    "A GPU address that this shader cannot write through; unlike ImmutableAddr, other shader pointers may write the buffer."
);
address!(
    ImmutableAddr,
    "A GPU address to a buffer never written by the GPU. CPU updates between frames are allowed. The shader Restrict contract requires no mutation during execution."
);

impl<T> From<Addr<T>> for ReadAddr<T> {
    fn from(address: Addr<T>) -> Self {
        Self::from_raw(address.to_raw())
    }
}

impl<T> From<ImmutableAddr<T>> for ReadAddr<T> {
    fn from(address: ImmutableAddr<T>) -> Self {
        Self::from_raw(address.to_raw())
    }
}
