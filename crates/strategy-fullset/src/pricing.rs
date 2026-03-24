use rust_decimal::{Decimal, RoundingStrategy};

pub type PricingResult<T> = Result<T, PricingError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullSetLeg {
    pub quantity: Decimal,
    pub price_usdc: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullSetFees {
    pub leg_fee_rate: Decimal,
    pub merge_fee_usdc: Decimal,
    pub split_fee_usdc: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuantizationPolicy {
    usdc_dp: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingBreakdown {
    pub gross_usdc: Decimal,
    pub input_cost_usdc: Decimal,
    pub fee_usdc_equiv: Decimal,
    pub rounding_loss_usdc: Decimal,
    pub net_output_usdc: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEdge {
    pub breakdown: PricingBreakdown,
    pub net_edge_usdc: Decimal,
    pub net_edge_bps: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PricingError {
    QuantityMismatch {
        yes_quantity: Decimal,
        no_quantity: Decimal,
    },
}

impl QuantizationPolicy {
    pub fn usdc_cents() -> Self {
        Self { usdc_dp: 2 }
    }

    fn quantize(self, value: Decimal) -> Decimal {
        value.round_dp_with_strategy(self.usdc_dp, RoundingStrategy::ToZero)
    }

    fn quantize_fee(self, value: Decimal) -> Decimal {
        value.round_dp_with_strategy(self.usdc_dp, RoundingStrategy::ToPositiveInfinity)
    }
}

impl FullSetLeg {
    fn notional(self) -> Decimal {
        self.quantity * self.price_usdc
    }
}

pub fn evaluate_buy_yes_buy_no_merge(
    yes_leg: FullSetLeg,
    no_leg: FullSetLeg,
    fees: FullSetFees,
    quantization: QuantizationPolicy,
) -> PricingResult<NormalizedEdge> {
    let gross_usdc = matched_quantity(yes_leg, no_leg)?;
    let input_cost_usdc = yes_leg.notional() + no_leg.notional();
    let fee_usdc_equiv = quantized_leg_fees(yes_leg, no_leg, fees.leg_fee_rate, quantization)
        + quantization.quantize_fee(fees.merge_fee_usdc);
    let net_output_usdc = quantization.quantize(gross_usdc);
    let rounding_loss_usdc = gross_usdc - net_output_usdc;

    Ok(build_result(
        gross_usdc,
        input_cost_usdc,
        fee_usdc_equiv,
        rounding_loss_usdc,
        net_output_usdc,
    ))
}

pub fn evaluate_split_sell_yes_sell_no(
    yes_leg: FullSetLeg,
    no_leg: FullSetLeg,
    fees: FullSetFees,
    quantization: QuantizationPolicy,
) -> PricingResult<NormalizedEdge> {
    let input_cost_usdc = matched_quantity(yes_leg, no_leg)?;
    let gross_usdc = yes_leg.notional() + no_leg.notional();
    let quantized_yes_output = quantization.quantize(yes_leg.notional());
    let quantized_no_output = quantization.quantize(no_leg.notional());
    let net_output_usdc = quantized_yes_output + quantized_no_output;
    let fee_usdc_equiv = quantized_leg_fees(yes_leg, no_leg, fees.leg_fee_rate, quantization)
        + quantization.quantize_fee(fees.split_fee_usdc);
    let rounding_loss_usdc = gross_usdc - net_output_usdc;

    Ok(build_result(
        gross_usdc,
        input_cost_usdc,
        fee_usdc_equiv,
        rounding_loss_usdc,
        net_output_usdc,
    ))
}

fn quantized_leg_fees(
    yes_leg: FullSetLeg,
    no_leg: FullSetLeg,
    leg_fee_rate: Decimal,
    quantization: QuantizationPolicy,
) -> Decimal {
    quantization.quantize_fee(yes_leg.notional() * leg_fee_rate)
        + quantization.quantize_fee(no_leg.notional() * leg_fee_rate)
}

fn matched_quantity(yes_leg: FullSetLeg, no_leg: FullSetLeg) -> PricingResult<Decimal> {
    if yes_leg.quantity != no_leg.quantity {
        return Err(PricingError::QuantityMismatch {
            yes_quantity: yes_leg.quantity,
            no_quantity: no_leg.quantity,
        });
    }

    Ok(yes_leg.quantity)
}

fn build_result(
    gross_usdc: Decimal,
    input_cost_usdc: Decimal,
    fee_usdc_equiv: Decimal,
    rounding_loss_usdc: Decimal,
    net_output_usdc: Decimal,
) -> NormalizedEdge {
    let net_edge_usdc = net_output_usdc - input_cost_usdc - fee_usdc_equiv;
    let net_edge_bps = if input_cost_usdc.is_zero() {
        Decimal::ZERO
    } else {
        (net_edge_usdc * Decimal::new(10_000, 0)) / input_cost_usdc
    };

    NormalizedEdge {
        breakdown: PricingBreakdown {
            gross_usdc,
            input_cost_usdc,
            fee_usdc_equiv,
            rounding_loss_usdc,
            net_output_usdc,
        },
        net_edge_usdc,
        net_edge_bps,
    }
}
