// inventory: e290_epilogue_exo_det
var y;
varexo e;
varexo_det ed;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e + ed;
end;
epilogue;
foo = ed;
end;
