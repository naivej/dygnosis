// inventory: pin72_quiet_e111_two_shocks_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
shocks;
var e; stderr .1;
end;
shocks;
var e; stderr .2;
end;
