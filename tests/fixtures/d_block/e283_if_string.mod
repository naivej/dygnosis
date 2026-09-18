// inventory: e283_if_string
var y;
varexo e;
parameters rho;
rho = 0.9;
@#if "hello"
@#endif
model;
y = rho * y(-1) + e;
end;
