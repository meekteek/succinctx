package vars

import (
	"encoding/binary"

	"github.com/consensys/gnark/frontend"
)

// The zero byte as a variable in a circuit. If used within APIs, it will be treated as a constant.
var ZERO_BYTE = Byte{Value: 0}

// A variable in a circuit representing a byte. Under the hood, the value is a single field element.
type Byte struct {
	Value frontend.Variable
}

// Creates a new byte as a variable in a circuit.
func NewByte(i1 byte) Byte {
	return Byte{Value: int(i1)}
}

// Creates a new array of bytes as a variable in a circuit.
func NewBytes(i1 []byte) []Byte {
	var result []Byte
	for i := 0; i < len(i1); i++ {
		result = append(result, Byte{Value: int(i1[i])})
	}
	return result
}

// Creates a new array of bytes32 as a variable in a circuit.
func NewBytesArray(i1 [][]byte) [][]Byte {
	var result [][]Byte
	for i := 0; i < len(i1); i++ {
		result = append(result, NewBytes(i1[i]))
	}
	return result
}

// Creates a new bytes32 as a variable in a circuit.
func NewBytes32(i1 [32]byte) [32]Byte {
	var result [32]Byte
	for i := 0; i < 32; i++ {
		result[i] = Byte{Value: int(i1[i])}
	}
	return result
}

// Creates a new array of bytes32 as a variable in a circuit.
func NewBytes32Array(i1 [][32]byte) [][32]Byte {
	var result [][32]Byte
	for i := 0; i < len(i1); i++ {
		result = append(result, NewBytes32(i1[i]))
	}
	return result
}

// Creates a new bytes32 as a variable in a circuit from a u64. The u64 will placed in the first
// 8 bytes of the bytes32 (aka "little endian").
func NewBytes32FromU64LE(i1 uint64) [32]Byte {
	var b [32]byte
	binary.LittleEndian.PutUint64(b[:], i1)
	return NewBytes32(b)
}

// Creates a new bytes32 as a variable in a circuit from a u64. The u64 will placed in the
func NewBytes32FromBytesLeftPad(i1 []byte) [32]Byte {
	var b [32]byte
	startOffset := 32 - len(i1) - 1
	for i := 0; i < len(i1); i++ {
		b[startOffset+i] = i1[i]
	}
	return NewBytes32(b)
}