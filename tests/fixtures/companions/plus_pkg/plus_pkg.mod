// inventory: companions_plus_pkg
// Generated +FILENAME/steadystate.m is not the user SS companion.
var y;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;
