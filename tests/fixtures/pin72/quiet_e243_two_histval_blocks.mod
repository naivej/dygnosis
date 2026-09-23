// inventory: pin72_quiet_e243_two_histval_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
histval;
y(0) = 1;
end;
histval;
y(0) = 2;
end;
