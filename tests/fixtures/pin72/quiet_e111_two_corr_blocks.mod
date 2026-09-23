// inventory: pin72_quiet_e111_two_corr_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
shocks;
corr e,e2 = .1;
end;
shocks;
corr e,e2 = .2;
end;
