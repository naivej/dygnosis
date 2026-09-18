// inventory: w201_restriction_fname
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
sbvar(restriction_fname=foo);
