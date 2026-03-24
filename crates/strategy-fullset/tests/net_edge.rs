use rust_decimal::Decimal;
use strategy_fullset::pricing::{
    evaluate_buy_yes_buy_no_merge, evaluate_split_sell_yes_sell_no, FullSetFees, FullSetLeg,
    QuantizationPolicy,
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
    );

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
            split_fee_usdc: Decimal::ZERO,
        },
        QuantizationPolicy::usdc_cents(),
    );

    assert_eq!(result.breakdown.gross_usdc, Decimal::new(201, 2));
    assert_eq!(result.breakdown.input_cost_usdc, Decimal::new(300, 2));
    assert_eq!(result.breakdown.rounding_loss_usdc, Decimal::new(1, 2));
    assert_eq!(result.breakdown.net_output_usdc, Decimal::new(200, 2));
    assert_eq!(result.net_edge_usdc, Decimal::new(-100, 2));
}
