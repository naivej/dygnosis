// inventory: e063_defined_before_use
@#define UNDEF = 1
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + @{UNDEF};
end;
