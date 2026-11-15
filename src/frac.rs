use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Sub};

// Time values are in whole notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frac {
    num: i64,
    den: i64,
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub fn lcm(a: i64, b: i64) -> i64 {
    a / gcd(a, b) * b
}

impl Frac {
    pub const ZERO: Frac = Frac { num: 0, den: 1 };

    pub fn new(num: i64, den: i64) -> Frac {
        assert!(den != 0, "fraction with zero denominator: {num}/0");
        let g = gcd(num, den).max(1);
        let sign = if den < 0 { -1 } else { 1 };
        Frac {
            num: sign * num / g,
            den: sign * den / g,
        }
    }

    pub fn int(n: i64) -> Frac {
        Frac { num: n, den: 1 }
    }

    pub fn num(self) -> i64 {
        self.num
    }

    pub fn den(self) -> i64 {
        self.den
    }

    pub fn is_zero(self) -> bool {
        self.num == 0
    }

    pub fn is_int(self) -> bool {
        self.den == 1
    }

    pub fn log2(self) -> Option<i32> {
        let is_pow2 = |n: i64| n > 0 && n & (n - 1) == 0;
        if self.num == 1 && is_pow2(self.den) {
            return Some(-(self.den.trailing_zeros() as i32));
        }
        if self.den == 1 && is_pow2(self.num) {
            return Some(self.num.trailing_zeros() as i32);
        }
        None
    }

    pub fn is_pow2(self) -> bool {
        self.log2().is_some()
    }

    pub fn gcd(self, other: Frac) -> Frac {
        Frac::new(
            gcd(self.num * other.den, other.num * self.den),
            self.den * other.den,
        )
    }
}

impl Default for Frac {
    fn default() -> Frac {
        Frac::ZERO
    }
}

impl fmt::Display for Frac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl Ord for Frac {
    fn cmp(&self, other: &Frac) -> Ordering {
        (i128::from(self.num) * i128::from(other.den))
            .cmp(&(i128::from(other.num) * i128::from(self.den)))
    }
}

impl PartialOrd for Frac {
    fn partial_cmp(&self, other: &Frac) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Add for Frac {
    type Output = Frac;
    fn add(self, o: Frac) -> Frac {
        let d = lcm(self.den, o.den);
        Frac::new(self.num * (d / self.den) + o.num * (d / o.den), d)
    }
}

impl AddAssign for Frac {
    fn add_assign(&mut self, o: Frac) {
        *self = *self + o;
    }
}

impl Sub for Frac {
    type Output = Frac;
    fn sub(self, o: Frac) -> Frac {
        self + Frac::new(-o.num, o.den)
    }
}

impl Mul for Frac {
    type Output = Frac;
    fn mul(self, o: Frac) -> Frac {
        let g1 = gcd(self.num, o.den).max(1);
        let g2 = gcd(o.num, self.den).max(1);
        Frac::new(
            (self.num / g1) * (o.num / g2),
            (self.den / g2) * (o.den / g1),
        )
    }
}

impl Div for Frac {
    type Output = Frac;
    fn div(self, o: Frac) -> Frac {
        assert!(o.num != 0, "division of {self} by zero");
        self * Frac::new(o.den, o.num)
    }
}

impl Mul<i64> for Frac {
    type Output = Frac;
    fn mul(self, k: i64) -> Frac {
        self * Frac::int(k)
    }
}

impl Div<i64> for Frac {
    type Output = Frac;
    fn div(self, k: i64) -> Frac {
        self / Frac::int(k)
    }
}

impl std::iter::Sum for Frac {
    fn sum<I: Iterator<Item = Frac>>(iter: I) -> Frac {
        iter.fold(Frac::ZERO, |a, b| a + b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteValue {
    pub log2: i32,
    pub dots: u8,
}

impl NoteValue {
    pub fn frac(self) -> Frac {
        let length = if self.log2 >= 0 {
            Frac::int(1 << self.log2)
        } else {
            Frac::new(1, 1 << -self.log2)
        };
        length * Frac::new((1 << (self.dots + 1)) - 1, 1 << self.dots)
    }

    pub fn of(d: Frac) -> Option<NoteValue> {
        for dots in 0..=3u8 {
            let factor = Frac::new((1 << (dots + 1)) - 1, 1 << dots);
            if let Some(log2) = (d / factor).log2() {
                return Some(NoteValue { log2, dots });
            }
        }
        None
    }
}
