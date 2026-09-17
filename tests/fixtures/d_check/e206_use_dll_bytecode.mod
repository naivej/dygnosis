// inventory: e206_use_dll_bytecode
var y;
varexo e;
parameters rho;
rho = 0.9;
model(use_dll, bytecode);
y = rho * y(-1) + e;
end;
