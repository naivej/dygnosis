// inventory: e063_undef_simple
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + @{UNDEF};
end;
