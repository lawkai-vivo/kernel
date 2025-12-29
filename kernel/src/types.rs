// Copyright (c) 2025 vivo Mobile Communication Co., Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//       http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

pub mod unique_owner_list;
use crate::sync::{ISpinLock, SpinLock, SpinLockGuard};
pub use blueos_infra::{
    impl_simple_intrusive_adapter,
    intrusive::{
        Adapter as IntrusiveAdapter, Nested as NestedAdapter, Relative as RelativeAdapter,
    },
    list::{
        typed_atomic_ilist::AtomicListHead as AtomicIlistHead,
        typed_ilist::{
            IouListHeadMut as IouIlistHeadMut, List as Ilist, ListHead as IlistHead,
            ListHeadIterator as IlistHeadIterator, ListIterator as IlistIterator,
        },
        GenericList,
    },
    tinyarc::{
        TinyArc as Arc, TinyArcCas as ArcCas, TinyArcInner as ArcInner, TinyArcList as ArcList,
        TinyArcListIterator as ArcListIterator,
    },
    tinyrwlock::{IRwLock, RwLock, RwLockReadGuard, RwLockWriteGuard},
};
use core::marker::PhantomData;
pub use unique_owner_list::{
    StaticListOwner, UniqueOwnerListAccessGuard, UniqueOwnerListHead, UniqueOwnerListIterator,
};

#[cfg(target_pointer_width = "32")]
mod inner {
    pub type Uint = u8;
    pub type AtomicUint = core::sync::atomic::AtomicU8;
    pub type Int = i8;
    pub type AtomicInt = core::sync::atomic::AtomicI8;
}

#[cfg(target_pointer_width = "64")]
mod inner {
    pub type Uint = usize;
    pub type Int = isize;
    pub type AtomicUint = core::sync::atomic::AtomicUsize;
    pub type AtomicInt = core::sync::atomic::AtomicIsize;
}

pub type ThreadPriority = u32;

pub use inner::*;

#[macro_export]
macro_rules! static_arc {
    ($name:ident($ty:ty, $val:expr),) => {
        #[allow(non_snake_case)]
        mod $name {
            use super::*;
            use $crate::types::{Arc, ArcInner};
            static CTRL_BLOCK: ArcInner<$ty> = ArcInner::new($val);
            pub(super) static PTR: Arc<$ty> = unsafe { Arc::from_static_inner_ref(&CTRL_BLOCK) };
        }
        use $name::PTR as $name;
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueos_test_macro::test;

    impl_simple_intrusive_adapter!(Node, Foobar, node);
    impl_simple_intrusive_adapter!(Lock, Foobar, node_lock);

    #[allow(clippy::type_complexity)]
    struct Foobar {
        node: AtomicIlistHead<Foobar, Node>,
        node_lock: ISpinLock<
            AtomicIlistHead<Foobar, Node>,
            RelativeAdapter<Foobar, Lock, Node, AtomicIlistHead<Foobar, Node>>,
        >,
    }

    #[test]
    fn test_intrusive_mutex_list_head() {
        type L = AtomicIlistHead<Foobar, Node>;
        let head = Arc::new(Foobar {
            node: AtomicIlistHead::new(),
            node_lock: ISpinLock::new(),
        });
        let a = Arc::new(Foobar {
            node: AtomicIlistHead::new(),
            node_lock: ISpinLock::new(),
        });
        let b = Arc::new(Foobar {
            node: AtomicIlistHead::new(),
            node_lock: ISpinLock::new(),
        });
        // For following insertions, the head doesn't get the share of ownership.
        let mut head_lock = head.node_lock.irqsave_lock();
        {
            let mut lock_a = a.node_lock.irqsave_lock();
            L::insert_after(&mut head_lock, &mut lock_a);
            drop(lock_a);
            assert_eq!(Arc::<Foobar>::strong_count(&a), 1);
        }
        {
            let mut lock_b = b.node_lock.irqsave_lock();
            L::insert_after(&mut head_lock, &mut lock_b);
        }
    }
}
