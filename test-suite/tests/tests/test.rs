#![no_std]
#![no_main]

use test_suite as _;

#[defmt_test::tests]
mod tests {
    #[test]
    fn assert_true() {
        assert!(true);
    }

    // #[test]
    // fn assert_false() {
    //     assert!(false);
    // }
}
