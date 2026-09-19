// inventory: w031_epilogue_dup
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
epilogue;
foo = 1;
foo = 2;
end;
