// Copyright 2024 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: Apache-2.0

use crate::coconut::bandwidth::freepass::FreePassIssuedData;
use crate::coconut::bandwidth::issuance::{
    BandwidthCredentialIssuanceDataVariant, IssuanceBandwidthCredential,
};
use crate::coconut::bandwidth::voucher::BandwidthVoucherIssuedData;
use crate::coconut::bandwidth::{bandwidth_voucher_params, CredentialSpendingData, CredentialType};
use crate::error::Error;
use nym_coconut_interface::{
    prove_bandwidth_credential, Parameters, PrivateAttribute, PublicAttribute, Signature,
    VerificationKey,
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Zeroize, ZeroizeOnDrop, Serialize, Deserialize)]
pub enum BandwidthCredentialIssuedDataVariant {
    Voucher(BandwidthVoucherIssuedData),
    FreePass(FreePassIssuedData),
}

impl<'a> From<&'a BandwidthCredentialIssuanceDataVariant> for BandwidthCredentialIssuedDataVariant {
    fn from(value: &'a BandwidthCredentialIssuanceDataVariant) -> Self {
        match value {
            BandwidthCredentialIssuanceDataVariant::Voucher(voucher) => {
                BandwidthCredentialIssuedDataVariant::Voucher(voucher.into())
            }
            BandwidthCredentialIssuanceDataVariant::FreePass(freepass) => {
                BandwidthCredentialIssuedDataVariant::FreePass(freepass.into())
            }
        }
    }
}

impl From<FreePassIssuedData> for BandwidthCredentialIssuedDataVariant {
    fn from(value: FreePassIssuedData) -> Self {
        BandwidthCredentialIssuedDataVariant::FreePass(value)
    }
}

impl From<BandwidthVoucherIssuedData> for BandwidthCredentialIssuedDataVariant {
    fn from(value: BandwidthVoucherIssuedData) -> Self {
        BandwidthCredentialIssuedDataVariant::Voucher(value)
    }
}

impl BandwidthCredentialIssuedDataVariant {
    pub fn info(&self) -> CredentialType {
        match self {
            BandwidthCredentialIssuedDataVariant::Voucher(..) => CredentialType::Voucher,
            BandwidthCredentialIssuedDataVariant::FreePass(..) => CredentialType::FreePass,
        }
    }

    // currently this works under the assumption of there being a single unique public attribute for given variant
    pub fn public_value_plain(&self) -> String {
        match self {
            BandwidthCredentialIssuedDataVariant::Voucher(voucher) => voucher.value_plain(),
            BandwidthCredentialIssuedDataVariant::FreePass(freepass) => {
                freepass.expiry_date_plain()
            }
        }
    }
}

// the only important thing to zeroize here are the private attributes, the rest can be made fully public for what we're concerned
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct IssuedBandwidthCredential {
    // private attributes
    /// a random secret value generated by the client used for double-spending detection
    serial_number: PrivateAttribute,

    /// a random secret value generated by the client used to bind multiple credentials together
    binding_number: PrivateAttribute,

    /// the underlying aggregated signature on the attributes
    #[zeroize(skip)]
    signature: Signature,

    /// data specific to given bandwidth credential, for example a value for bandwidth voucher and expiry date for the free pass
    variant_data: BandwidthCredentialIssuedDataVariant,

    /// type of the bandwdith credential hashed onto a scalar
    type_prehashed: PublicAttribute,
}

impl IssuedBandwidthCredential {
    pub fn new(
        serial_number: PrivateAttribute,
        binding_number: PrivateAttribute,
        signature: Signature,
        variant_data: BandwidthCredentialIssuedDataVariant,
        type_prehashed: PublicAttribute,
    ) -> Self {
        IssuedBandwidthCredential {
            serial_number,
            binding_number,
            signature,
            variant_data,
            type_prehashed,
        }
    }

    pub fn default_parameters() -> Parameters {
        IssuanceBandwidthCredential::default_parameters()
    }

    pub fn typ(&self) -> CredentialType {
        self.variant_data.info()
    }

    pub fn get_plain_public_attributes(&self) -> Vec<String> {
        vec![
            self.variant_data.public_value_plain(),
            self.typ().to_string(),
        ]
    }

    pub fn prepare_for_spending(
        &self,
        verification_key: &VerificationKey,
    ) -> Result<CredentialSpendingData, Error> {
        let params = bandwidth_voucher_params();

        let verify_credential_request = prove_bandwidth_credential(
            params,
            verification_key,
            &self.signature,
            &self.serial_number,
            &self.binding_number,
        )?;

        Ok(CredentialSpendingData {
            verify_credential_request,
            public_attributes_plain: self.get_plain_public_attributes(),
        })
    }
}
