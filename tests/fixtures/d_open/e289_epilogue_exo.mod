// inventory: e289_epilogue_exo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
epilogue;
foo = e;
end;
