// inventory: pin72_quiet_e273_two_generate_irfs_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
generate_irfs;
a,e = 1;
end;
generate_irfs;
a,e = 2;
end;
