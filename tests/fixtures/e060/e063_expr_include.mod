// inventory: e063_expr_with_include
@#include "e063_params.inc"
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + @{UNDEF+1};
end;
