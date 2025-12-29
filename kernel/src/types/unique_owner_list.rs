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

use crate::{
    sync::{SpinLock, SpinLockGuard},
    types::{Arc, Ilist, IlistHead, IlistHeadIterator, IntrusiveAdapter},
};
use core::marker::PhantomData;

// Every type implements this trait corresponds to a 'static variable so that
// UniqueOwnerListHead doesn't need to store its owner.
#[const_trait]
pub trait StaticListOwner<T, A: IntrusiveAdapter<T>> {
    type List = IlistHead<T, A>;
    fn get() -> &'static Arc<SpinLock<IlistHead<T, A>>>;
}

#[derive(Debug, Default)]
pub struct UniqueOwnerListHead<T, A: IntrusiveAdapter<T>, O: StaticListOwner<T, A>>(
    IlistHead<T, A>,
    PhantomData<O>,
);

pub struct UniqueOwnerListIterator<'a, T, A: IntrusiveAdapter<T>> {
    inner: IlistHeadIterator<T, A>,
    _a: PhantomData<&'a ()>,
}

impl<'a, T: 'a, A: IntrusiveAdapter<T> + 'a> Iterator for UniqueOwnerListIterator<'a, T, A> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.inner.next()?;
        Some(unsafe { node.as_ref().owner() })
    }
}

pub struct UniqueOwnerListAccessGuard<
    T: 'static,
    A: IntrusiveAdapter<T> + 'static,
    O: StaticListOwner<T, A>,
>(SpinLockGuard<'static, IlistHead<T, A>>, PhantomData<O>);

impl<T: 'static, A: IntrusiveAdapter<T> + 'static, O: StaticListOwner<T, A>>
    UniqueOwnerListAccessGuard<T, A, O>
{
    #[inline]
    pub fn new() -> Self {
        let w = O::get().irqsave_lock();
        Self(w, PhantomData)
    }

    #[inline]
    pub fn detach(&mut self, me: &mut Arc<T>) -> bool {
        UniqueOwnerListHead::<T, A, O>::inner_detach(me)
    }

    #[inline]
    pub fn insert(&mut self, me: Arc<T>) -> bool {
        UniqueOwnerListHead::<T, A, O>::inner_insert(&mut self.0, me)
    }

    #[inline]
    pub fn get_list_mut(&mut self) -> &mut IlistHead<T, A> {
        &mut self.0
    }

    #[inline]
    pub fn get_guard_mut(&mut self) -> &mut SpinLockGuard<'static, IlistHead<T, A>> {
        &mut self.0
    }

    #[inline]
    pub fn iter(&self) -> UniqueOwnerListIterator<'_, T, A> {
        UniqueOwnerListIterator {
            inner: IlistHeadIterator::new(&self.0, None),
            _a: PhantomData,
        }
    }
}

impl<T: 'static, A: IntrusiveAdapter<T> + 'static, O: StaticListOwner<T, A>>
    UniqueOwnerListHead<T, A, O>
{
    pub const fn new() -> Self {
        Self(IlistHead::<T, A>::new(), PhantomData)
    }

    #[inline]
    pub fn lock() -> UniqueOwnerListAccessGuard<T, A, O> {
        UniqueOwnerListAccessGuard::new()
    }

    fn inner_detach(me: &mut Arc<T>) -> bool {
        let node = unsafe { Ilist::<T, A>::list_head_of_mut(Arc::get_mut_unchecked(me)) };
        if !IlistHead::detach(node) {
            return false;
        }
        unsafe { Arc::decrement_strong_count(me) };
        true
    }

    #[inline]
    pub fn detach(me: &mut Arc<T>) -> bool {
        let _guard = O::get().irqsave_lock();
        Self::inner_detach(me)
    }

    fn inner_insert(head: &mut IlistHead<T, A>, mut me: Arc<T>) -> bool {
        let node = unsafe { Ilist::<T, A>::list_head_of_mut(Arc::get_mut_unchecked(&mut me)) };
        if !IlistHead::insert_after(head, node) {
            return false;
        }
        core::mem::forget(me);
        true
    }

    #[inline]
    pub fn insert(me: Arc<T>) -> bool {
        let mut head = O::get().irqsave_lock();
        Self::inner_insert(&mut head, me)
    }
}
