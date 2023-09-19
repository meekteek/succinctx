use itertools::Itertools;

use super::tree::MerkleInclusionProofVariable;
use crate::backend::circuit::PlonkParameters;
use crate::frontend::vars::Bytes32Variable;
use crate::prelude::{
    ArrayVariable, BoolVariable, ByteVariable, BytesVariable, CircuitBuilder, CircuitVariable,
};

/// Merkle Tree implementation for the Tendermint spec (follows Comet BFT Simple Merkle Tree spec: https://docs.cometbft.com/main/spec/core/encoding#merkle-trees).
/// TODO: Create generic interface for Merkle trees to implement.
impl<L: PlonkParameters<D>, const D: usize> CircuitBuilder<L, D> {
    /// Leaf should already be hashed.
    pub fn get_root_from_merkle_proof_hashed_leaf<const PROOF_DEPTH: usize>(
        &mut self,
        aunts: &ArrayVariable<Bytes32Variable, PROOF_DEPTH>,
        path_indices: &ArrayVariable<BoolVariable, PROOF_DEPTH>,
        leaf: Bytes32Variable,
    ) -> Bytes32Variable {
        let mut hash_so_far = leaf;

        for i in 0..PROOF_DEPTH {
            let aunt = aunts[i];
            let path_index = path_indices[i];
            let left_hash_pair = self.inner_hash(&hash_so_far, &aunt);
            let right_hash_pair = self.inner_hash(&aunt, &hash_so_far);

            hash_so_far = self.select(path_index, right_hash_pair, left_hash_pair)
        }
        hash_so_far
    }

    pub fn get_root_from_merkle_proof<const PROOF_DEPTH: usize, const LEAF_SIZE_BYTES: usize>(
        &mut self,
        inclusion_proof: &MerkleInclusionProofVariable<PROOF_DEPTH, LEAF_SIZE_BYTES>,
    ) -> Bytes32Variable {
        let hashed_leaf = self.leaf_hash(&inclusion_proof.leaf.0);

        self.get_root_from_merkle_proof_hashed_leaf::<PROOF_DEPTH>(
            &inclusion_proof.aunts,
            &inclusion_proof.path_indices,
            hashed_leaf,
        )
    }

    pub fn leaf_hash(&mut self, leaf: &[ByteVariable]) -> Bytes32Variable {
        let zero_byte = ByteVariable::constant(self, 0u8);

        let mut encoded_leaf = vec![zero_byte];

        // Append the leaf bytes to the zero byte.
        encoded_leaf.extend(leaf.to_vec());

        // Load the output of the hash.
        // Use curta gadget to generate SHA's.
        // Note: This can be removed when sha256 interface is fixed.
        self.curta_sha256(&encoded_leaf)
    }

    pub fn inner_hash(
        &mut self,
        left: &Bytes32Variable,
        right: &Bytes32Variable,
    ) -> Bytes32Variable {
        // Calculate the length of the message for the inner hash.
        // 0x01 || left || right
        let one_byte = ByteVariable::constant(self, 1u8);

        let mut encoded_leaf = vec![one_byte];

        // Append the left bytes to the one byte.
        encoded_leaf.extend(left.as_bytes().to_vec());

        // Append the right bytes to the bytes so far.
        encoded_leaf.extend(right.as_bytes().to_vec());

        // Load the output of the hash.
        // Note: Calculate the inner hash as if both validators are enabled.
        self.curta_sha256(&encoded_leaf)
    }

    pub fn hash_merkle_layer(
        &mut self,
        merkle_hashes: Vec<Bytes32Variable>,
        merkle_hash_enabled: Vec<BoolVariable>,
        num_hashes: usize,
    ) -> (Vec<Bytes32Variable>, Vec<BoolVariable>) {
        let zero = self._false();
        let one = self._true();

        let mut new_merkle_hashes = Vec::new();
        let mut new_merkle_hash_enabled = Vec::new();

        for i in (0..num_hashes).step_by(2) {
            let both_nodes_enabled = self.and(merkle_hash_enabled[i], merkle_hash_enabled[i + 1]);

            let first_node_disabled = self.not(merkle_hash_enabled[i]);
            let second_node_disabled = self.not(merkle_hash_enabled[i + 1]);
            let both_nodes_disabled = self.and(first_node_disabled, second_node_disabled);

            // Calculuate the inner hash.
            let inner_hash = self.inner_hash(&merkle_hashes[i], &merkle_hashes[i + 1]);

            new_merkle_hashes.push(self.select(both_nodes_enabled, inner_hash, merkle_hashes[i]));

            // Set the inner node one level up to disabled if both nodes are disabled.
            new_merkle_hash_enabled.push(self.select(both_nodes_disabled, zero, one));
        }

        // Return the hashes and enabled nodes for the next layer up.
        (new_merkle_hashes, new_merkle_hash_enabled)
    }

    pub fn hash_leaves<const LEAF_SIZE_BYTES: usize>(
        &mut self,
        leaves: Vec<BytesVariable<LEAF_SIZE_BYTES>>,
    ) -> Vec<Bytes32Variable> {
        leaves
            .iter()
            .map(|leaf| self.leaf_hash(&leaf.0))
            .collect_vec()
    }

    pub fn get_root_from_hashed_leaves<const NB_LEAVES: usize>(
        &mut self,
        leaf_hashes: Vec<Bytes32Variable>,
        leaves_enabled: Vec<BoolVariable>,
    ) -> Bytes32Variable {
        assert!(NB_LEAVES.is_power_of_two());
        assert!(leaf_hashes.len() == NB_LEAVES);
        assert!(leaves_enabled.len() == NB_LEAVES);

        // Hash each of the validators to get their corresponding leaf hash.
        let mut current_nodes = leaf_hashes.clone();

        // Whether to treat the validator as empty.
        let mut current_node_enabled = leaves_enabled.clone();

        let mut merkle_layer_size = NB_LEAVES;

        // Hash each layer of nodes to get the root according to the Tendermint spec, starting from the leaves.
        while merkle_layer_size > 1 {
            (current_nodes, current_node_enabled) =
                self.hash_merkle_layer(current_nodes, current_node_enabled, merkle_layer_size);
            merkle_layer_size /= 2;
        }

        // Return the root hash.
        current_nodes[0]
    }

    pub fn compute_root_from_leaves<const NB_LEAVES: usize, const LEAF_SIZE_BYTES: usize>(
        &mut self,
        leaves: Vec<BytesVariable<LEAF_SIZE_BYTES>>,
        leaves_enabled: Vec<BoolVariable>,
    ) -> Bytes32Variable {
        assert!(NB_LEAVES == leaves.len());
        assert!(NB_LEAVES == leaves_enabled.len());

        let hashed_leaves = self.hash_leaves::<LEAF_SIZE_BYTES>(leaves.to_vec());
        self.get_root_from_hashed_leaves::<NB_LEAVES>(hashed_leaves, leaves_enabled.to_vec())
    }
}

#[cfg(test)]
mod tests {

    use std::env;

    use ethers::types::H256;
    use itertools::Itertools;

    use crate::backend::circuit::DefaultParameters;
    use crate::frontend::merkle::tree::{InclusionProof, MerkleInclusionProofVariable};
    use crate::prelude::*;

    type L = DefaultParameters;
    type F = <L as PlonkParameters<D>>::Field;
    const D: usize = 2;

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_get_root_from_leaves() {
        env::set_var("RUST_LOG", "debug");
        env_logger::try_init().unwrap_or_default();
        dotenv::dotenv().ok();

        let mut builder = CircuitBuilder::<L, D>::new();

        let leaves = builder.read::<ArrayVariable<BytesVariable<48>, 32>>();
        let enabled = builder.read::<ArrayVariable<BoolVariable, 32>>();
        let root = builder.compute_root_from_leaves::<32, 48>(leaves.as_vec(), enabled.as_vec());
        builder.write::<Bytes32Variable>(root);
        let circuit = builder.build();
        circuit.test_default_serializers();

        let mut input = circuit.input();

        input.write::<ArrayVariable<BytesVariable<48>, 32>>([[0u8; 48]; 32].to_vec());
        input.write::<ArrayVariable<BoolVariable, 32>>([true; 32].to_vec());

        let (proof, mut output) = circuit.prove(&input);
        circuit.verify(&proof, &input, &output);
        let root = output.read::<Bytes32Variable>();

        assert_eq!(
            root,
            bytes32!("0xde8624485c0a1b8f9ecc858312916104cc3ee3ed601e405c11eaf9c5cbe05117"),
        );
    }

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_get_root_from_merkle_proof() {
        env::set_var("RUST_LOG", "debug");
        env_logger::try_init().unwrap_or_default();
        dotenv::dotenv().ok();

        let mut builder = CircuitBuilder::<L, D>::new();

        let proof_variable = builder.read::<MerkleInclusionProofVariable<4, 48>>();

        let root = builder.get_root_from_merkle_proof(&proof_variable);
        builder.write::<Bytes32Variable>(root);

        let circuit = builder.build();
        circuit.test_default_serializers();

        let mut input = circuit.input();

        let leaves = [[0u8; 48]; 16].to_vec();
        let aunts = [
            "78877fa898f0b4c45c9c33ae941e40617ad7c8657a307db62bc5691f92f4f60e",
            "8195d3a7e856bd9bf73464642c1e9177c7e0fbe9cf7458e2572f4e7c267676c7",
            "b1992b2f60fc8b11b83c6d9dbdd1d6abb1f5ef91c0a7aa4e7d629532048d0270",
            "0611fc80429feb4b56817f4070d289650ac0a8eaaa8975c8cc72b73e96376bff",
        ];
        let inclusion_proof: InclusionProof<4, 48, F> = InclusionProof {
            leaf: leaves[0],
            path_indices: vec![false; 4],
            aunts: aunts
                .iter()
                .map(|aunt| H256::from_slice(hex::decode(aunt).unwrap().as_slice()))
                .collect_vec(),
        };
        input.write::<MerkleInclusionProofVariable<4, 48>>(inclusion_proof);

        let (proof, mut output) = circuit.prove(&input);
        circuit.verify(&proof, &input, &output);

        let computed_root = output.read::<Bytes32Variable>();
        assert_eq!(
            bytes32!("50d7ed02b144a75487702c9f5faaea07bb9a7385e1521e80f6080399fb9a0ffd"),
            computed_root
        );
    }
}
