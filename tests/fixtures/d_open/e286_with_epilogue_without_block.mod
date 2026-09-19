// inventory: e286_with_epilogue_without_block
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shock_decomposition(with_epilogue);
