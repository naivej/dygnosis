// inventory: e172_missing_regime
var is i;
varexo e;
parameters rhos;
rhos = 0.8;
model;
is = rhos * is(-1) + e;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
bind i <= 0;
end;
shocks;
var e; stderr 0.01;
end;
