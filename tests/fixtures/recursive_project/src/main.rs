fn recurse() {
    recurse();
}

fn a() {
    b();
}

fn b() {
    a();
}

fn f01() {} fn f02() {} fn f03() {} fn f04() {} fn f05() {}
fn f06() {} fn f07() {} fn f08() {} fn f09() {} fn f10() {}
fn f11() {} fn f12() {} fn f13() {} fn f14() {} fn f15() {}
fn f16() {} fn f17() {} fn f18() {} fn f19() {} fn f20() {}
fn f21() {}

fn wide() {
    f21(); f20(); f19(); f18(); f17(); f16(); f15();
    f14(); f13(); f12(); f11(); f10(); f09(); f08();
    f07(); f06(); f05(); f04(); f03(); f02(); f01();
}

fn main() {
    recurse();
    a();
    wide();
}
