// inventory: quiet_epilogue
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
epilogue;
foo = y;
end;
shock_decomposition(with_epilogue);
