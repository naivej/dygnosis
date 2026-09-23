// inventory: pin72_quiet_e313_two_filter_initial_state_blocks
var y z;
varexo e e2;
parameters rho;
rho = .5;
model;
y = rho*y(-1) + e + e2;
z = y;
end;
filter_initial_state;
y(0) = 0;
end;
filter_initial_state;
y(0) = 1;
end;
