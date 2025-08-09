# Justfile for developer tasks

# Generate code from lexicons using esquema-cli
codegen:
	esquema-cli generate local -l ./examples/lexicons/ -o ./examples --module codegen
