use core::marker::PhantomData;

use plonky2::iop::generator::{GeneratedValues, SimpleGenerator};
use plonky2::iop::target::Target;
use plonky2::iop::witness::{PartitionWitness, WitnessWrite};
use plonky2::plonk::circuit_data::CommonCircuitData;
use plonky2::plonk::config::{AlgebraicHasher, GenericConfig};
use plonky2::plonk::proof::ProofWithPublicInputsTarget;
use plonky2::util::serialization::{Buffer, IoResult, Read, Write};

use super::{MapReduceInputVariable, MapReduceInputVariableValue};
use crate::backend::circuit::CircuitBuild;
use crate::backend::prover::EnvProver;
use crate::prelude::{CircuitVariable, GateRegistry, PlonkParameters, WitnessGeneratorRegistry};

#[derive(Debug, Clone)]
pub struct MapReduceGenerator<L, C, I, O, const D: usize>
where
    L: PlonkParameters<D>,
    <L as PlonkParameters<D>>::Config: GenericConfig<D, F = L::Field> + 'static,
    <<L as PlonkParameters<D>>::Config as GenericConfig<D>>::Hasher: AlgebraicHasher<L::Field>,
    C: CircuitVariable,
    I: CircuitVariable,
    O: CircuitVariable,
{
    /// The identifier for the compiled map circuit.
    pub map_circuit_id: String,

    /// The identifiers for the compiled reduce circuits.
    pub reduce_circuit_ids: Vec<String>,

    /// The global context for all circuits.
    pub ctx: C,

    /// The constant inputs to the map circuit.
    pub inputs: Vec<I::ValueType<L::Field>>,

    /// The proof target for the final circuit proof.
    pub proof: ProofWithPublicInputsTarget<D>,

    /// Phantom data.
    pub _phantom1: PhantomData<L>,
    pub _phantom2: PhantomData<O>,
}

impl<L, C, I, O, const D: usize> MapReduceGenerator<L, C, I, O, D>
where
    L: PlonkParameters<D>,
    <L as PlonkParameters<D>>::Config: GenericConfig<D, F = L::Field> + 'static,
    <<L as PlonkParameters<D>>::Config as GenericConfig<D>>::Hasher: AlgebraicHasher<L::Field>,
    C: CircuitVariable,
    I: CircuitVariable,
    O: CircuitVariable,
{
    pub fn id() -> String {
        "MapReduceGenerator".to_string()
    }
}

impl<L, C, I, O, const D: usize> SimpleGenerator<L::Field, D> for MapReduceGenerator<L, C, I, O, D>
where
    L: PlonkParameters<D>,
    <L as PlonkParameters<D>>::Config: GenericConfig<D, F = L::Field> + 'static,
    <<L as PlonkParameters<D>>::Config as GenericConfig<D>>::Hasher: AlgebraicHasher<L::Field>,
    C: CircuitVariable,
    I: CircuitVariable,
    O: CircuitVariable,
    <I as CircuitVariable>::ValueType<<L as PlonkParameters<D>>::Field>: Sync + Send,
{
    fn id(&self) -> String {
        Self::id()
    }

    fn dependencies(&self) -> Vec<Target> {
        let mut targets = Vec::new();
        targets.extend(self.ctx.targets());
        targets
    }

    fn run_once(
        &self,
        witness: &PartitionWitness<L::Field>,
        out_buffer: &mut GeneratedValues<L::Field>,
    ) {
        // The gate and witness generator serializers.
        let gate_serializer = GateRegistry::<L, D>::new();
        let generator_serializer = WitnessGeneratorRegistry::<L, D>::new();

        // Create the prover and the async runtime.
        let prover = EnvProver::new();

        // Load the map circuit from disk & generate the proofs.
        let map_circuit_path = format!("./build/{}.circuit", self.map_circuit_id);
        let map_circuit =
            CircuitBuild::<L, D>::load(&map_circuit_path, &gate_serializer, &generator_serializer)
                .unwrap();

        // Calculate the inputs to the map.
        let ctx_value = self.ctx.get(witness);
        let map_input_values = &self.inputs;
        let mut map_inputs = Vec::new();
        for map_input_value in map_input_values {
            let mut map_input = map_circuit.input();
            map_input.write::<MapReduceInputVariable<C, I>>(MapReduceInputVariableValue {
                ctx: ctx_value.clone(),
                input: map_input_value.to_owned(),
            });
            map_inputs.push(map_input)
        }

        // Generate the proofs for the map layer.
        let (mut proofs, _) = prover.batch_prove(&map_circuit, &map_inputs).unwrap();

        // Process each reduce layer.
        let nb_reduce_layers = (self.inputs.len() as f64).log2().ceil() as usize;
        for i in 0..nb_reduce_layers {
            // Load the reduce circuit from disk.
            let reduce_circuit_path = format!("./build/{}.circuit", self.reduce_circuit_ids[i]);
            let reduce_circuit = CircuitBuild::<L, D>::load(
                &reduce_circuit_path,
                &gate_serializer,
                &generator_serializer,
            )
            .unwrap();

            // Calculate the inputs to the reduce layer.
            let nb_proofs = self.inputs.len() / (2usize.pow((i + 1) as u32));
            let mut reduce_inputs = Vec::new();
            for j in 0..nb_proofs {
                let mut reduce_input = reduce_circuit.input();
                reduce_input.proof_write(proofs[j * 2].clone());
                reduce_input.proof_write(proofs[j * 2 + 1].clone());
                reduce_inputs.push(reduce_input);
            }

            // Generate the proofs for the reduce layer and update the proofs buffer.
            (proofs, _) = prover.batch_prove(&reduce_circuit, &reduce_inputs).unwrap();
        }

        // Set the proof target with the final proof.
        out_buffer.set_proof_with_pis_target(&self.proof, &proofs[0]);
    }

    fn serialize(&self, dst: &mut Vec<u8>, _: &CommonCircuitData<L::Field, D>) -> IoResult<()> {
        // Write map circuit.
        dst.write_usize(self.map_circuit_id.len())?;
        dst.write_all(self.map_circuit_id.as_bytes())?;

        // Write vector of reduce circuits.
        dst.write_usize(self.reduce_circuit_ids.len())?;
        for i in 0..self.reduce_circuit_ids.len() {
            dst.write_usize(self.reduce_circuit_ids[i].len())?;
            dst.write_all(self.reduce_circuit_ids[i].as_bytes())?;
        }

        // Write context.
        dst.write_target_vec(&self.ctx.targets())?;

        // Write vector of input values.
        dst.write_usize(self.inputs.len())?;
        for i in 0..self.inputs.len() {
            dst.write_field_vec::<L::Field>(&I::elements::<L, D>(self.inputs[i].clone()))?;
        }

        // Write proof target.
        dst.write_target_proof_with_public_inputs(&self.proof)
    }

    fn deserialize(src: &mut Buffer, _: &CommonCircuitData<L::Field, D>) -> IoResult<Self> {
        // Read map circuit.
        let map_circuit_id_length = src.read_usize()?;
        let mut map_circuit_id = vec![0u8; map_circuit_id_length];
        src.read_exact(&mut map_circuit_id)?;

        // Read vector of reduce circuits.
        let mut reduce_circuit_ids = Vec::new();
        let reduce_circuit_ids_len = src.read_usize()?;
        for _ in 0..reduce_circuit_ids_len {
            let reduce_circuit_id_length = src.read_usize()?;
            let mut reduce_circuit_id = vec![0u8; reduce_circuit_id_length];
            src.read_exact(&mut reduce_circuit_id)?;
            reduce_circuit_ids.push(String::from_utf8(reduce_circuit_id).unwrap());
        }

        // Read context.
        let ctx = C::from_targets(&src.read_target_vec()?);

        // Read vector of input targest.
        let mut inputs = Vec::new();
        let inputs_len = src.read_usize()?;
        for _ in 0..inputs_len {
            let input_elements: Vec<L::Field> = src.read_field_vec(I::nb_elements())?;
            inputs.push(I::from_elements::<L, D>(&input_elements));
        }

        // Read proof.
        let proof = src.read_target_proof_with_public_inputs()?;

        Ok(Self {
            map_circuit_id: String::from_utf8(map_circuit_id).unwrap(),
            reduce_circuit_ids,
            ctx,
            inputs,
            proof,
            _phantom1: PhantomData::<L>,
            _phantom2: PhantomData::<O>,
        })
    }
}