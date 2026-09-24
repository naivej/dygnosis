// inventory: d_pac_e453_growth_name_clash
var y x z v;
varexo ey ex ez;
parameters phi psi k beta p_pac_growth_neutrality_correction;
phi=.3; psi=.4; k=.5; beta=.9;
var_model(model_name=aux,eqtags=['Y','X']);
pac_model(model_name=p,auxiliary_model_name=aux,discount=beta,growth=v);
model;
  [name='Y'] y=phi*y(-1)+ey;
  [name='X'] diff(x)=psi*diff(x(-1))+ex;
  [name='V'] v=x+y;
  [name='P'] diff(z)=k*(v(-1)-z(-1))+psi*diff(z(-1))+pac_expectation(p)+ez;
end;
