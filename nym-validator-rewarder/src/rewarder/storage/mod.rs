// Copyright 2023 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use crate::error::NymRewarderError;
use crate::rewarder::epoch::Epoch;
use crate::rewarder::storage::manager::StorageManager;
use crate::rewarder::{EpochRewards, RewardingResult};
use nym_validator_client::nyxd::Coin;
use sqlx::ConnectOptions;
use std::fmt::Debug;
use std::path::Path;
use tracing::{error, info, instrument};

mod manager;

pub struct RewarderStorage {
    pub(crate) manager: StorageManager,
}

impl RewarderStorage {
    #[instrument]
    pub async fn init<P: AsRef<Path> + Debug>(database_path: P) -> Result<Self, NymRewarderError> {
        let mut opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true);

        // TODO: do we want auto_vacuum ?

        opts.disable_statement_logging();

        let connection_pool = match sqlx::SqlitePool::connect_with(opts).await {
            Ok(db) => db,
            Err(err) => {
                error!("Failed to connect to SQLx database: {err}");
                return Err(err.into());
            }
        };

        if let Err(err) = sqlx::migrate!("./migrations").run(&connection_pool).await {
            error!("Failed to initialize SQLx database: {err}");
            return Err(err.into());
        }

        info!("Database migration finished!");

        let manager = StorageManager { connection_pool };
        let storage = RewarderStorage { manager };

        Ok(storage)
    }

    pub(crate) async fn load_last_rewarding_epoch(
        &self,
    ) -> Result<Option<Epoch>, NymRewarderError> {
        Ok(self.manager.load_last_rewarding_epoch().await?)
    }

    async fn insert_failed_rewarding_epoch_block_signing(
        &self,
        epoch: i64,
        budget: &Coin,
    ) -> Result<(), NymRewarderError> {
        Ok(self
            .manager
            .insert_rewarding_epoch_block_signing(epoch, -1, -1, budget.to_string())
            .await?)
    }

    async fn insert_failed_rewarding_epoch_credential_issuance(
        &self,
        epoch: i64,
        budget: &Coin,
    ) -> Result<(), NymRewarderError> {
        Ok(self
            .manager
            .insert_rewarding_epoch_credential_issuance(epoch, -1, -1, -1, budget.to_string())
            .await?)
    }

    pub(crate) async fn save_rewarding_information(
        &self,
        reward: EpochRewards,
        rewarding_result: Result<RewardingResult, NymRewarderError>, // total_spent: Coin,
                                                                     // rewarding_tx: Result<Hash, NymRewarderError>,
    ) -> Result<(), NymRewarderError> {
        info!("persisting reward details");
        let denom = &reward.total_budget.denom;

        let (reward_tx, total_spent, reward_err) = match rewarding_result {
            Ok(res) => (Some(res.rewarding_tx.to_string()), res.total_spent, None),
            Err(err) => (None, Coin::new(0, denom), Some(err.to_string())),
        };

        let epoch_id = reward.epoch.id;

        // general epoch info
        self.manager
            .insert_rewarding_epoch(
                reward.epoch,
                reward.total_budget.to_string(),
                total_spent.to_string(),
                reward_tx,
                reward_err,
            )
            .await?;

        // block signing info
        if let Ok(block_signing) = reward.signing {
            self.manager
                .insert_rewarding_epoch_block_signing(
                    epoch_id,
                    block_signing
                        .as_ref()
                        .map(|s| s.total_voting_power_at_epoch_start)
                        .unwrap_or_default(),
                    block_signing.as_ref().map(|s| s.blocks).unwrap_or_default(),
                    reward.signing_budget.to_string(),
                )
                .await?;
            if let Some(signing) = block_signing {
                for validator in signing.validators {
                    let reward_amount = validator.reward_amount(&reward.signing_budget).to_string();
                    self.manager
                        .insert_rewarding_epoch_block_signing_reward(
                            epoch_id,
                            validator.validator.consensus_address,
                            validator.operator_account.to_string(),
                            validator.whitelisted,
                            reward_amount,
                            validator.voting_power_at_epoch_start,
                            validator.voting_power_ratio.to_string(),
                            validator.signed_blocks,
                            validator.ratio_signed.to_string(),
                        )
                        .await?;
                }
            }
        } else {
            self.insert_failed_rewarding_epoch_block_signing(epoch_id, &reward.signing_budget)
                .await?;
        }

        // credential info
        if let Ok(credential_issuance) = reward.credentials {
            // safety: we must have at least a single value here
            #[allow(clippy::unwrap_used)]
            let dkg_epoch_start = credential_issuance
                .as_ref()
                .and_then(|c| c.dkg_epochs.first().copied())
                .unwrap_or_default() as i64;
            #[allow(clippy::unwrap_used)]
            let dkg_epoch_end = credential_issuance
                .as_ref()
                .and_then(|c| c.dkg_epochs.last().copied())
                .unwrap_or_default() as i64;

            self.manager
                .insert_rewarding_epoch_credential_issuance(
                    epoch_id,
                    dkg_epoch_start,
                    dkg_epoch_end,
                    credential_issuance
                        .as_ref()
                        .map(|c| c.total_issued_partial_credentials)
                        .unwrap_or_default() as i64,
                    reward.credentials_budget.to_string(),
                )
                .await?;

            if let Some(credentials) = credential_issuance {
                for api_runner in credentials.api_runners {
                    let reward_amount = api_runner
                        .reward_amount(&reward.credentials_budget)
                        .to_string();

                    self.manager
                        .insert_rewarding_epoch_credential_issuance_reward(
                            epoch_id,
                            api_runner.runner_account.to_string(),
                            api_runner.whitelisted,
                            reward_amount,
                            api_runner.api_runner,
                            api_runner.issued_credentials,
                            api_runner.issued_ratio.to_string(),
                            api_runner.validated_credentials,
                        )
                        .await?;
                }
            }
        } else {
            self.insert_failed_rewarding_epoch_credential_issuance(
                epoch_id,
                &reward.credentials_budget,
            )
            .await?;
        }

        Ok(())
    }
}
