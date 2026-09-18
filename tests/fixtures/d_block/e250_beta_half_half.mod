// inventory: e250_beta_half_half
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
estimated_params;
rho, 0.5, 0, 1, beta_pdf, 0.5, 0.5;
end;
