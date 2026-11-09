package com.limelight.binding.input.virtual_controller;

import java.util.Arrays;

public class ButtonTouchOwnersTest {
    private static void check(boolean v) { if (!v) throw new AssertionError(); }
    private static void ids(ButtonTouchOwners o, int... expected) { check(Arrays.equals(o.ids(), expected)); }
    public static void main(String[] args) {
        ButtonTouchOwners o = new ButtonTouchOwners();
        check(o.down(3, true)); check(!o.down(7, false)); ids(o, 3);
        check(o.up(3)); ids(o); check(!o.up(7));
        check(o.down(4, true)); check(!o.down(9, true)); ids(o, 4, 9);
        check(!o.up(4)); ids(o, 9); check(o.up(9));
        check(o.down(1, true)); check(!o.down(2, true)); check(!o.up(2)); check(o.up(1));
        check(o.down(1, true)); check(o.cancel()); ids(o); check(!o.cancel()); check(!o.up(1));
        check(!o.down(8, false)); check(o.down(9, true)); check(o.up(9));
        System.out.println("PASS: button touch ownership");
    }
}
