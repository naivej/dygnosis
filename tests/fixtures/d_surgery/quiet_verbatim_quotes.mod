// inventory: d_surgery_quiet_verbatim_quotes
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho*y(-1)+e;
end;
shocks;
var e = 0.01;
end;
initval;
y = 1;
end;
verbatim;
"raw text passes through, double quotes and all"
end;