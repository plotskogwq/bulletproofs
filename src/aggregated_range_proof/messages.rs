use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use inner_product_proof;

use curve25519_dalek::ristretto;
use curve25519_dalek::traits::IsIdentity;
use proof_transcript::ProofTranscript;
use rand::Rng;
use std::iter;
use util;

#[derive(Clone)]
pub struct ValueCommitment {
    pub V: RistrettoPoint,
    pub A: RistrettoPoint,
    pub S: RistrettoPoint,
}

pub struct ValueChallenge {
    pub y: Scalar,
    pub z: Scalar,
}

#[derive(Clone)]
pub struct PolyCommitment {
    pub T_1: RistrettoPoint,
    pub T_2: RistrettoPoint,
}

pub struct PolyChallenge {
    pub x: Scalar,
}

#[derive(Clone)]
pub struct ProofShare {
    pub value_commitment: ValueCommitment,
    pub poly_commitment: PolyCommitment,

    pub t_x: Scalar,
    pub t_x_blinding: Scalar,
    pub e_blinding: Scalar,

    pub l_vec: Vec<Scalar>,
    pub r_vec: Vec<Scalar>,
}

pub struct Proof {
    pub n: usize,
    /// Commitment to the value
    // XXX this should not be included, so that we can prove about existing commitments
    // included for now so that it's easier to test
    pub value_commitments: Vec<RistrettoPoint>,
    /// Commitment to the bits of the value
    pub A: RistrettoPoint,
    /// Commitment to the blinding factors
    pub S: RistrettoPoint,
    /// Commitment to the \\(t_1\\) coefficient of \\( t(x) \\)
    pub T_1: RistrettoPoint,
    /// Commitment to the \\(t_2\\) coefficient of \\( t(x) \\)
    pub T_2: RistrettoPoint,
    /// Evaluation of the polynomial \\(t(x)\\) at the challenge point \\(x\\)
    pub t_x: Scalar,
    /// Blinding factor for the synthetic commitment to \\(t(x)\\)
    pub t_x_blinding: Scalar,
    /// Blinding factor for the synthetic commitment to the inner-product arguments
    pub e_blinding: Scalar,
    /// Proof data for the inner-product argument.
    pub ipp_proof: inner_product_proof::InnerProductProof,
}

impl Proof {
    pub fn verify<R: Rng>(&self, rng: &mut R, transcript: &mut ProofTranscript) -> Result<(), ()> {
        use generators::{Generators, PedersenGenerators};

        let n = self.n;
        let m = self.value_commitments.len();

        let generators = Generators::new(PedersenGenerators::default(), n, m);
        let gen = generators.all();

        transcript.commit_u64(n as u64);
        transcript.commit_u64(m as u64);

        for V in self.value_commitments.iter() {
            transcript.commit(V.compress().as_bytes());
        }
        transcript.commit(self.A.compress().as_bytes());
        transcript.commit(self.S.compress().as_bytes());

        let y = transcript.challenge_scalar();
        let z = transcript.challenge_scalar();

        transcript.commit(self.T_1.compress().as_bytes());
        transcript.commit(self.T_2.compress().as_bytes());

        let x = transcript.challenge_scalar();

        transcript.commit(self.t_x.as_bytes());
        transcript.commit(self.t_x_blinding.as_bytes());
        transcript.commit(self.e_blinding.as_bytes());

        let w = transcript.challenge_scalar();
        let zz = z * z;
        let minus_z = -z;

        // Challenge value for batching statements to be verified
        let c = Scalar::random(rng);

        let (x_sq, x_inv_sq, s) = self.ipp_proof.verification_scalars(transcript);

        let s_inv = s.iter().rev();

        let a = self.ipp_proof.a;
        let b = self.ipp_proof.b;

        let g = s.iter().map(|s_i| minus_z - a * s_i);

        // Compute product in updated P
        // z^0 * \vec(2)^n || z^1 * \vec(2)^n || ... || z^(m-1) * \vec(2)^n
        let powers_of_2: Vec<Scalar> = util::exp_iter(Scalar::from_u64(2)).take(n).collect();
        let powers_of_z = util::exp_iter(z).take(m);
        let concat_z_and_2 =
            powers_of_z.flat_map(|exp_z| powers_of_2.iter().map(move |exp_2| exp_2 * exp_z));

        let h = s_inv
            .zip(util::exp_iter(y.invert()))
            .zip(concat_z_and_2)
            .map(|((s_i_inv, exp_y_inv), z_and_2)| z + exp_y_inv * (zz * z_and_2 - b * s_i_inv));

        let value_commitment_scalars = util::exp_iter(z).take(m).map(|z_exp| c * zz * z_exp);
        let basepoint_scalar = w * (self.t_x - a * b) + c * (delta(n, m, &y, &z) - self.t_x);

        let mega_check = ristretto::vartime::multiscalar_mul(
            iter::once(Scalar::one())
                .chain(iter::once(x))
                .chain(value_commitment_scalars)
                .chain(iter::once(c * x))
                .chain(iter::once(c * x * x))
                .chain(iter::once(-self.e_blinding - c * self.t_x_blinding))
                .chain(iter::once(basepoint_scalar))
                .chain(g)
                .chain(h)
                .chain(x_sq.iter().cloned())
                .chain(x_inv_sq.iter().cloned()),
            iter::once(&self.A)
                .chain(iter::once(&self.S))
                .chain(self.value_commitments.iter())
                .chain(iter::once(&self.T_1))
                .chain(iter::once(&self.T_2))
                .chain(iter::once(&gen.pedersen_generators.B_blinding))
                .chain(iter::once(&gen.pedersen_generators.B))
                .chain(gen.G.iter())
                .chain(gen.H.iter())
                .chain(self.ipp_proof.L_vec.iter())
                .chain(self.ipp_proof.R_vec.iter()),
        );

        if mega_check.is_identity() {
            Ok(())
        } else {
            Err(())
        }
    }
}

/// Compute delta(y,z) = (z - z^2)<1^n*m, y^n*m> + z^3 <1, 2^n*m> * \sum_j=0^(m-1) z^j
fn delta(n: usize, m: usize, y: &Scalar, z: &Scalar) -> Scalar {
    let two = Scalar::from_u64(2);

    // XXX this could be more efficient, esp for powers of 2
    let sum_of_powers_of_y = util::exp_iter(*y)
        .take(n * m)
        .fold(Scalar::zero(), |acc, x| acc + x);

    // XXX TODO: just calculate (2^n - 1) instead
    let sum_of_powers_of_2 = util::exp_iter(two)
        .take(n)
        .fold(Scalar::zero(), |acc, x| acc + x);

    let sum_of_powers_of_z = util::exp_iter(*z)
        .take(m)
        .fold(Scalar::zero(), |acc, x| acc + x);

    let zz = z * z;

    (z - zz) * sum_of_powers_of_y - z * zz * sum_of_powers_of_2 * sum_of_powers_of_z
}
