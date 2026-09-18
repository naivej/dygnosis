// inventory: e285_plus_type
var y;
varexo e;
parameters rho;
rho = 0.9;
@#define x = "a"
@#define yy = x + 1
model;
y = rho * y(-1) + e;
end;
