// inventory: companions_ident_helper
// Ident helper call outside model; sibling helper .m is present.
var y;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;

steady_state_model;
y = my_ss_helper(rho);
end;
