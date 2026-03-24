use rust_decimal::Decimal;
use strategy_fullset::pricing::{
    evaluate_buy_yes_buy_no_merge, evaluate_split_sell_yes_sell_no, FullSetFees, FullSetLeg,
    PricingError, QuantizationPolicy,
};

#[test]
fn net_edge_uses_usdc_normalization_and_fee_rounding() {
    let result = evaluate_buy_yes_buy_no_merge(
        FullSetLeg {
            quantity: Decimal::new(3, 0),
            price_usdc: Decimal::new(49, 2),
        },
        FullSetLeg {
            quantity: Decimal::new(3, 0),
            price_usdc: Decimal::new(48, 2),
        },
        FullSetFees {
            leg_fee_rate: Decimal::new(2, 2),
            merge_fee_usdc: Decimal::new(3, 2),
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::usdc_cents(),
    )
    .expect("matched legs should price");

    assert_eq!(result.breakdown.gross_usdc, Decimal::new(300, 2));
    assert_eq!(result.breakdown.input_cost_usdc, Decimal::new(291, 2));
    assert_eq!(result.breakdown.fee_usdc_equiv, Decimal::new(9, 2));
    assert_eq!(result.breakdown.rounding_loss_usdc, Decimal::new(0, 2));
    assert_eq!(result.breakdown.net_output_usdc, Decimal::new(300, 2));
    assert_eq!(result.net_edge_usdc, Decimal::ZERO);
}

#[test]
fn rounding_loss_is_recorded_in_breakdown() {
    let result = evaluate_split_sell_yes_sell_no(
        FullSetLeg {
            quantity: Decimal::new(3, 0),
            price_usdc: Decimal::new(335, 3),
        },
        FullSetLeg {
            quantity: Decimal::new(3, 0),
            price_usdc: Decimal::new(335, 3),
        },
        FullSetFees {
            leg_fee_rate: Decimal::ZERO,
            merge_fee_usdc: Decimal::ZERO,
            split_fee_usdc: Decimal::new(2, 2),
        },
        QuantizationPolicy::usdc_cents(),
    )
    .expect("matched legs should price");

    assert_eq!(result.breakdown.gross_usdc, Decimal::new(201, 2));
    assert_eq!(result.breakdown.input_cost_usdc, Decimal::new(300, 2));
    assert_eq!(result.breakdown.fee_usdc_equiv, Decimal::new(2, 2));
    assert_eq!(result.breakdown.rounding_loss_usdc, Decimal::new(1, 2));
    assert_eq!(result.breakdown.net_output_usdc, Decimal::new(200, 2));
    assert_eq!(result.net_edge_usdc, Decimal::new(-102, 2));
}

#[test]
fn buy_merge_rejects_mismatched_leg_quantities() {
    let result = evaluate_buy_yes_buy_no_merge(
        FullSetLeg {
            quantity: Decimal::new(3, 0),
            price_usdc: Decimal::new(49, 2),
        },
        FullSetLeg {
            quantity: Decimal::new(2, 0),
            price_usdc: Decimal::new(48, 2),
        },
        FullSetFees {
            leg_fee_rate: Decimal::ZERO,
            merge_fee_usdc: Decimal::ZERO,
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::usdc_cents(),
    );

    assert!(result.is_err(), "mismatched full-set legs must be rejected");
}

#[test]
fn buy_merge_rejects_negative_quantity() {
    let result = evaluate_buy_yes_buy_no_merge(
        FullSetLeg {
            quantity: Decimal::new(-1, 0),
            price_usdc: Decimal::new(49, 2),
        },
        FullSetLeg {
            quantity: Decimal::new(-1, 0),
            price_usdc: Decimal::new(48, 2),
        },
        FullSetFees {
            leg_fee_rate: Decimal::ZERO,
            merge_fee_usdc: Decimal::ZERO,
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::usdc_cents(),
    );

    assert_eq!(
        result,
        Err(PricingError::NegativeQuantity {
            leg: "YES",
            quantity: Decimal::new(-1, 0),
        })
    );
}

#[test]
fn buy_merge_rejects_negative_price() {
    let result = evaluate_buy_yes_buy_no_merge(
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(-1, 1),
        },
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(48, 2),
        },
        FullSetFees {
            leg_fee_rate: Decimal::ZERO,
            merge_fee_usdc: Decimal::ZERO,
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::usdc_cents(),
    );

    assert_eq!(
        result,
        Err(PricingError::NegativePrice {
            leg: "YES",
            price_usdc: Decimal::new(-1, 1),
        })
    );
}

#[test]
fn buy_merge_rejects_price_above_one_usdc() {
    let result = evaluate_buy_yes_buy_no_merge(
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(1001, 3),
        },
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(48, 2),
        },
        FullSetFees {
            leg_fee_rate: Decimal::ZERO,
            merge_fee_usdc: Decimal::ZERO,
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::usdc_cents(),
    );

    assert_eq!(
        result,
        Err(PricingError::PriceAboveOne {
            leg: "YES",
            price_usdc: Decimal::new(1001, 3),
        })
    );
}

#[test]
fn split_sell_rejects_price_off_tick() {
    let result = evaluate_split_sell_yes_sell_no(
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(3335, 4),
        },
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(66, 2),
        },
        FullSetFees {
            leg_fee_rate: Decimal::ZERO,
            merge_fee_usdc: Decimal::ZERO,
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::usdc_cents(),
    );

    assert_eq!(
        result,
        Err(PricingError::PriceOffTick {
            leg: "YES",
            price_usdc: Decimal::new(3335, 4),
            price_quantum: Decimal::new(1, 3),
        })
    );
}

#[test]
fn split_sell_accepts_price_on_custom_tick() {
    let result = evaluate_split_sell_yes_sell_no(
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(33, 2),
        },
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(66, 2),
        },
        FullSetFees {
            leg_fee_rate: Decimal::ZERO,
            merge_fee_usdc: Decimal::ZERO,
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::with_price_quantum(Decimal::new(1, 2))
            .expect("positive tick should construct policy"),
    );

    assert!(
        result.is_ok(),
        "custom on-tick prices should price successfully"
    );
}

#[test]
fn split_sell_rejects_price_off_custom_tick() {
    let result = evaluate_split_sell_yes_sell_no(
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(333, 3),
        },
        FullSetLeg {
            quantity: Decimal::new(1, 0),
            price_usdc: Decimal::new(66, 2),
        },
        FullSetFees {
            leg_fee_rate: Decimal::ZERO,
            merge_fee_usdc: Decimal::ZERO,
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::with_price_quantum(Decimal::new(1, 2))
            .expect("positive tick should construct policy"),
    );

    assert_eq!(
        result,
        Err(PricingError::PriceOffTick {
            leg: "YES",
            price_usdc: Decimal::new(333, 3),
            price_quantum: Decimal::new(1, 2),
        })
    );
}

#[test]
fn with_price_quantum_rejects_zero() {
    assert_eq!(
        QuantizationPolicy::with_price_quantum(Decimal::ZERO),
        Err(PricingError::InvalidPriceQuantum {
            price_quantum: Decimal::ZERO,
        })
    );
}

#[test]
fn with_price_quantum_rejects_negative_values() {
    assert_eq!(
        QuantizationPolicy::with_price_quantum(Decimal::new(-1, 2)),
        Err(PricingError::InvalidPriceQuantum {
            price_quantum: Decimal::new(-1, 2),
        })
    );
}
