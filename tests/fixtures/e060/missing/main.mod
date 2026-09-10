// inventory: e061_missing_sibling
@#include "no_such_file.inc"
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
