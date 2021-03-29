// This file is part of Substrate.

// Copyright (C) 2017-2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! # Assets Freezer Pallet
//!
//! An extension pallet for use with the Assets pallet for allowing funds to be locked and reserved.

// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]
/*
pub mod weights;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
pub mod mock;
#[cfg(test)]
mod tests;
*/
use sp_std::prelude::*;
use sp_runtime::{
	RuntimeDebug, TokenError, traits::{
		AtLeast32BitUnsigned, Zero, StaticLookup, Saturating, CheckedSub, CheckedAdd,
		StoredMapError,
	}
};
use codec::{Encode, Decode, HasCompact};
use frame_support::{ensure, dispatch::{DispatchError, DispatchResult}};
use frame_support::traits::{
	Currency, ReservableCurrency, BalanceStatus::Reserved, StoredMap, tokens::{
		WithdrawConsequence, DepositConsequence, fungibles, FrozenBalance, WhenDust
}};
use frame_system::Config as SystemConfig;

//pub use weights::WeightInfo;
pub use pallet::*;
use frame_benchmarking::frame_support::dispatch::result::Result::{Err, Ok};
use frame_benchmarking::frame_support::traits::fungibles::{Unbalanced, Inspect};

type BalanceOf<T> = <<T as Config>::Assets as fungibles::Inspect<<T as SystemConfig>::AccountId>>::Balance;
type AssetIdOf<T> = <<T as Config>::Assets as fungibles::Inspect<<T as SystemConfig>::AccountId>>::AssetId;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::{
		dispatch::DispatchResult,
		pallet_prelude::*,
	};
	use frame_system::pallet_prelude::*;
	use super::*;

	/// The information concerning our freezing.
	#[derive(Eq, PartialEq, Clone, Encode, Decode, RuntimeDebug, Default)]
	pub struct FreezeData<Balance> {
		/// The amount of funds that have been reserved. The actual amount of funds held in reserve
		/// (and thus guaranteed of being unreserved) is this amount less `melted`.
		///
		/// If this `is_zero`, then the account may be deleted. If it is non-zero, then the assets
		/// pallet will attempt to keep the account alive by retaining the `minimum_balance` *plus*
		/// this number of funds in it.
		pub(super) reserved: Balance,
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	/// The module configuration trait.
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// The fungibles trait impl whose assets this reserves.
		type Assets: fungibles::Inspect<Self::AccountId>;

		/// Place to store the fast-access freeze data for the given asset/account.
		type Store: StoredMap<(AssetIdOf<Self>, Self::AccountId), FreezeData<BalanceOf<Self>>>;

//		/// Weight information for extrinsics in this pallet.
//		type WeightInfo: WeightInfo;
	}

	//
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	#[pallet::metadata(T::AccountId = "AccountId", BalanceOf<T> = "Balance", AssetIdOf<T> = "AssetId")]
	pub enum Event<T: Config> {
		/// An asset has been reserved.
		/// \[asset, who, amount\]
		Held(AssetIdOf<T>, T::AccountId, BalanceOf<T>),
		/// An asset has been unreserved.
		/// \[asset, who, amount\]
		Released(AssetIdOf<T>, T::AccountId, BalanceOf<T>),
	}

	// No new errors
	#[pallet::error]
	pub enum Error<T> {}

	// No hooks.
	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	// Only admin calls.
	#[pallet::call]
	impl<T: Config> Pallet<T> {}
}

impl<T: Config> FrozenBalance<AssetIdOf<T>, T::AccountId, BalanceOf<T>> for Pallet<T> {
	fn frozen_balance(id: AssetIdOf<T>, who: &T::AccountId) -> Option<BalanceOf<T>> {
		let f = T::Store::get(&(id, who.clone()));
		if f.reserved.is_zero() { None } else { Some(f.reserved) }
	}
}

impl<T: Config> fungibles::Inspect<<T as SystemConfig>::AccountId> for Pallet<T> {
	type AssetId = AssetIdOf<T>;
	type Balance = BalanceOf<T>;
	fn total_issuance(asset: AssetIdOf<T>) -> BalanceOf<T> {
		T::Assets::total_issuance(asset)
	}
	fn minimum_balance(asset: AssetIdOf<T>) -> BalanceOf<T> {
		T::Assets::minimum_balance(asset)
	}
	fn balance(asset: AssetIdOf<T>, who: &T::AccountId) -> BalanceOf<T> {
		T::Assets::balance(asset, who)
	}
	fn reducible_balance(asset: AssetIdOf<T>, who: &T::AccountId, keep_alive: bool) -> BalanceOf<T> {
		T::Assets::reducible_balance(asset, who, keep_alive)
	}
	fn can_deposit(asset: AssetIdOf<T>, who: &T::AccountId, amount: BalanceOf<T>)
		-> DepositConsequence
	{
		T::Assets::can_deposit(asset, who, amount)
	}
	fn can_withdraw(
		asset: AssetIdOf<T>,
		who: &T::AccountId,
		amount: BalanceOf<T>,
	) -> WithdrawConsequence<BalanceOf<T>> {
		T::Assets::can_withdraw(asset, who, amount)
	}
}

impl<T: Config> fungibles::Transfer<<T as SystemConfig>::AccountId> for Pallet<T> where
	T::Assets: fungibles::Transfer<T::AccountId>
{
	fn transfer(
		asset: Self::AssetId,
		source: &T::AccountId,
		dest: &T::AccountId,
		amount: Self::Balance,
		death: WhenDust,
	) -> Result<Self::Balance, DispatchError> {
		T::Assets::transfer(asset, source, dest, amount, death)
	}
	fn transfer_best_effort(
		asset: Self::AssetId,
		source: &T::AccountId,
		dest: &T::AccountId,
		amount: Self::Balance,
		death: WhenDust,
	) -> Result<Self::Balance, DispatchError> {
		T::Assets::transfer_best_effort(asset, source, dest, amount, death)
	}
}

impl<T: Config> fungibles::Unbalanced<T::AccountId> for Pallet<T> where
	T::Assets: fungibles::Unbalanced<T::AccountId>
{
	fn set_balance(asset: Self::AssetId, who: &T::AccountId, amount: Self::Balance)
		-> DispatchResult
	{
		T::Assets::set_balance(asset, who, amount)
	}

	fn set_total_issuance(asset: Self::AssetId, amount: Self::Balance) {
		T::Assets::set_total_issuance(asset, amount)
	}

	fn decrease_balance(
		asset: Self::AssetId,
		who: &T::AccountId,
		amount: Self::Balance,
		keep_alive: bool,
	) -> Result<Self::Balance, DispatchError> {
		T::Assets::decrease_balance(asset, who, amount, keep_alive)
	}

	fn increase_balance(asset: Self::AssetId, who: &T::AccountId, amount: Self::Balance)
		-> Result<(), DispatchError>
	{
		T::Assets::increase_balance(asset, who, amount)
	}

	fn decrease_balance_at_most(
		asset: Self::AssetId,
		who: &T::AccountId,
		amount: Self::Balance,
		keep_alive: bool,
	) -> Self::Balance {
		T::Assets::decrease_balance_at_most(asset, who, amount, keep_alive)
	}

	fn increase_balance_at_most(
		asset: Self::AssetId,
		who: &T::AccountId,
		amount: Self::Balance,
	) -> Self::Balance {
		T::Assets::increase_balance_at_most(asset, who, amount)
	}
}

impl<T: Config> fungibles::InspectHold<<T as SystemConfig>::AccountId> for Pallet<T> {
	fn balance_on_hold(asset: AssetIdOf<T>, who: &T::AccountId) -> BalanceOf<T> {
		T::Store::get(&(asset, who.clone())).reserved
	}
	fn can_hold(asset: AssetIdOf<T>, who: &T::AccountId, amount: BalanceOf<T>) -> bool {
		// If we can withdraw without destroying the account, then we're good.
		<Self as fungibles::Inspect<T::AccountId>>::can_withdraw(asset, who, amount) == WithdrawConsequence::Success
	}
}

impl<T: Config> fungibles::MutateHold<<T as SystemConfig>::AccountId> for Pallet<T> where
	T::Assets: fungibles::Transfer<T::AccountId> + fungibles::InspectWithoutFreezer<T::AccountId>
{
	fn hold(asset: AssetIdOf<T>, who: &T::AccountId, amount: BalanceOf<T>) -> DispatchResult {
		use fungibles::InspectHold;
		if !Self::can_hold(asset, who, amount) {
			Err(TokenError::NoFunds)?
		}
		T::Store::mutate(
			&(asset, who.clone()),
			|extra| extra.reserved = extra.reserved.saturating_add(amount),
		)?;
		Ok(())
	}

	fn release(asset: AssetIdOf<T>, who: &T::AccountId, amount: BalanceOf<T>, best_effort: bool)
		-> Result<BalanceOf<T>, DispatchError>
	{
		T::Store::try_mutate_exists(
			&(asset, who.clone()),
			|maybe_extra| if let Some(ref mut extra) = maybe_extra {
				let old = extra.reserved;
				extra.reserved = extra.reserved.saturating_sub(amount);
				let actual = old - extra.reserved;
				ensure!(best_effort || actual == amount, TokenError::NoFunds);
				Ok(actual)
			} else {
				Err(TokenError::NoFunds)?
			},
		)
	}

	fn transfer_held(
		asset: AssetIdOf<T>,
		source: &T::AccountId,
		dest: &T::AccountId,
		amount: BalanceOf<T>,
		best_effort: bool,
		on_hold: bool,
	) -> Result<BalanceOf<T>, DispatchError> {
		// Can't create the account with just a chunk of held balance - there needs to already be
		// the minimum deposit.
		let min_balance = <Self as fungibles::Inspect<_>> ::minimum_balance(asset);
		let dest_balance = <Self as fungibles::Inspect<_>>::balance(asset, dest);
		ensure!(!on_hold || dest_balance >= min_balance, TokenError::CannotCreate);

		let actual = Self::decrease_on_hold(asset, source, amount, best_effort)?;

		// `death` is `KeepAlive` here since we're only transferring funds that were on hold, for
		// which there must be an additional min_balance, it should be impossible for the transfer
		// to cause the account to be deleted.
		use fungibles::Transfer;
		let result = Self::transfer(asset, source, dest, actual, WhenDust::KeepAlive);
		if result.is_ok() {
			if on_hold {
				let r = T::Store::mutate(
					&(asset, dest.clone()),
					|extra| extra.reserved = extra.reserved.saturating_add(actual),
				);
				debug_assert!(r.is_ok(), "account exists and funds transferred in; qed");
				r?
			}
		} else {
			debug_assert!(false, "can_withdraw was successful; qed");
		}
		result
	}
}

impl<T: Config> fungibles::UnbalancedHold<<T as SystemConfig>::AccountId> for Pallet<T> where
	T::Assets: fungibles::Unbalanced<T::AccountId>
{
	fn decrease_balance_on_hold(
		asset: AssetIdOf<T>,
		source: &T::AccountId,
		amount: BalanceOf<T>,
		best_effort: bool,
	) -> Result<BalanceOf<T>, DispatchError> {
		let actual = Self::decrease_on_hold(asset, source, amount, best_effort)?;
		// The previous call's success guarantees the next will succeed.
		<Self as fungibles::Unbalanced<_>>::decrease_balance(asset, source, actual, true)
	}
}

impl<T: Config> Pallet<T> {
	/// Reduce the amount we have on hold of an account in such a way to ensure that the balance
	/// should be decreasable by the amount reduced.
	///
	/// NOTE: This won't alter the balance of the account.
	fn decrease_on_hold(
		asset: AssetIdOf<T>,
		source: &T::AccountId,
		amount: BalanceOf<T>,
		best_effort: bool,
	) -> Result<BalanceOf<T>, DispatchError> {
		use fungibles::{InspectHold, Transfer, InspectWithoutFreezer};

		let min_balance = <Self as fungibles::Inspect<_>>::minimum_balance(asset);

		T::Store::try_mutate_exists(
			&(asset, source.clone()),
			|maybe_extra| -> Result<BalanceOf<T>, DispatchError>
		{
			if let Some(ref mut extra) = maybe_extra {
				// Figure out the most we can unreserve and transfer.
				let old = extra.reserved;
				extra.reserved = extra.reserved.saturating_sub(amount);
				let actual = old - extra.reserved;
				ensure!(best_effort || actual == amount, TokenError::NoFunds);

				// actual is how much we can unreserve. now we check that the balance actually
				// exists in the account.
				let balance_left = <Self as fungibles::Inspect<_>>::balance(asset, source)
					.saturating_sub(min_balance);
				ensure!(balance_left >= actual, TokenError::NoFunds);

				// the balance for the reserved amount actually exists. now we check that it's
				// possible to actually transfer it out. practically, the only reason this would
				// fail is if the asset class or account is completely frozen.
				<Self as fungibles::Inspect<_>>::can_withdraw(asset, source, actual)
					.into_result(true)?;

				// all good. we should now be able to unreserve and transfer without any error.
				Ok(actual)
			} else {
				Err(TokenError::NoFunds)?
			}
		})
	}
}