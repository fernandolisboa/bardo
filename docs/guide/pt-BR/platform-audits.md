---
id: platform-audits
title: Auditorias e revisões das plataformas
group: publishing
---

# Auditorias e revisões das plataformas

Cada rede pode auditar ou revisar o app que você registrou para o Bardo. Como esse app é seu e atende só às suas próprias contas, quase nada disso se aplica: o Bardo envia, agenda e lê seus números sem auditoria nenhuma. Esta página diz, rede por rede, quando uma revisão é necessária, o que ela pede, o que o Bardo já oferece para ela e o que muda depois da aprovação. Nada aqui é preciso para usar o Bardo; é para quando você decidir fazer.

Em [Configurações › Redes](bardo:go/settings/networks), o botão **Auditorias e revisões** de cada cartão de app abre a parte da rede desta página como uma tela de passo a passo.

<a id="summary"></a>
## Você precisa de uma?

| Rede | Precisa hoje? | O que a revisão pede | O que muda no Bardo |
| --- | --- | --- | --- |
| YouTube | Só para cota acima do padrão e, talvez, para envios públicos (veja [Mantido privado](#youtube-private)) | Cumprir as Políticas para Desenvolvedores do YouTube, uma política de privacidade, capturas de tela e uma conta de demonstração | Nada no código; a cota do seu projeto |
| TikTok | Não, e nem passaria: ferramentas pessoais são recusadas. Os rascunhos na sua caixa de entrada funcionam pelo sandbox | Um site público com política de privacidade e termos, um vídeo de demonstração do fluxo inteiro, um motivo por escopo | Nada, a não ser que o Bardo tenha um app próprio |
| Instagram | Não: o Acesso Padrão atende às contas que você tem ou administra | Revisão do App (uma gravação de tela por permissão) e Verificação da Empresa, para o Acesso Avançado | Nada, a não ser que contas que você não administra devam se conectar |
| X | Não: o X não revisa apps; o acesso é pago por uso, com créditos | Aceitar o Developer Agreement and Policy e uma descrição curta do seu uso | Nada |

Fatos conferidos em 13/10/2026 (o X em 14/10/2026) na documentação de cada rede, com os links em cada seção.

<a id="youtube-need"></a>
## YouTube: você precisa da auditoria?

A página de [cota e auditorias de conformidade](https://developers.google.com/youtube/v3/guides/quota_and_compliance_audits) do Google dá a todo projeto que ativa a YouTube Data API 100 chamadas de `videos.insert` (envios), 100 de `search.list` e 10.000 unidades para todo o resto, por dia. A auditoria é como um projeto pede mais. Com um canal e poucos envios por dia, o padrão sobra: um envio custa 1 unidade de um balde próprio, e acompanhar, agendar e sincronizar métricas custam poucas unidades cada.

Ela pode valer a pena se:

- você envia mais de 100 vídeos por dia de um mesmo projeto, ou faz mais de 100 pesquisas de nicho por dia (cada nicho custa uma chamada de `search.list`);
- todo envio volta como **Mantido privado** (veja a próxima seção).

A auditoria é separada da **verificação do app OAuth** do Google, que trata da tela de consentimento. Um app para [uso pessoal, com menos de 100 usuários](https://support.google.com/cloud/answer/13464323) não precisa de verificação; você passa uma vez pela tela de "app não verificado", como descreve a conexão do YouTube.

<a id="youtube-private"></a>
## Se o YouTube manteve um vídeo privado

O Bardo mostra **Mantido privado** quando o YouTube deixa o vídeo como privado embora você tenha escolhido público ou não listado (num vídeo agendado, ainda privado quinze minutos depois do horário). O YouTube não diz o motivo, então o post diz só que o YouTube o manteve privado. As causas conhecidas até agora:

1. **A regra de 2020 para projetos não auditados.** Desde 28/07/2020, o YouTube trava como privados os envios de projetos de API não verificados criados depois dessa data, até o projeto passar pela auditoria. As páginas do Google hoje discordam: a referência de [`videos.insert`](https://developers.google.com/youtube/v3/docs/videos/insert), atualizada em 08/10/2026, diz que vídeos enviados de projetos não verificados não ficam restritos ao modo privado, enquanto o [histórico de revisões](https://developers.google.com/youtube/v3/revision_history) ainda traz a regra de 2020 e nenhuma entrada que a suspenda. Se todo envio do Bardo fica privado, seu projeto está sob essa regra.
2. **Uma mudança feita no YouTube Studio.** Você (ou outro administrador do canal) deixou o vídeo privado ou cancelou o agendamento antes de o Bardo conferir.
3. **Uma restrição que o YouTube aplicou ao vídeo.** Abra o vídeo no [YouTube Studio](https://studio.youtube.com/): as colunas **Visibilidade** e **Restrições** mostram o que o YouTube aplicou e por quê.

Em todos os casos o vídeo está no YouTube: deixe-o público no YouTube Studio. Sob a regra de 2020, os próximos envios continuam privados até a auditoria passar.

<a id="youtube-asks"></a>
## YouTube: o que a auditoria pede

A auditoria confere se o app segue os [Termos e Políticas para Desenvolvedores dos Serviços de API do YouTube](https://developers.google.com/youtube/terms/developer-policies). O [formulário de auditoria e extensão de cota](https://support.google.com/youtube/contact/yt_api_form) pede:

- quem você é: pessoa física ou empresa, nome, endereço, site e contatos;
- como você trabalha com o YouTube: uma descrição do negócio (de 100 a 5.000 caracteres), o público e como ele ganha dinheiro;
- o cliente de API: o nome, o endereço onde é usado, uma **URL de política de privacidade** (obrigatória), uma URL de termos de serviço e uma conta de demonstração com instruções para o revisor;
- por projeto do Google Cloud: o **número do projeto**, o caso de uso (para o Bardo, "Video Uploading & Account Management"), os métodos que ele chama, a cota que quer para `videos.insert` e `search.list` com o motivo, e capturas de tela da política de privacidade, da página inicial com o link para ela, do fluxo de login e da tela de envio.

**O que o Bardo tem para isso.** As políticas pedem uma política de privacidade que cite os Serviços de API do YouTube e aponte para a do Google, um link para os Termos de Serviço do YouTube no app, e dados do YouTube guardados atualizados ou apagados em até 30 dias. A [política de privacidade](https://fernandolisboa.github.io/bardo/pt-BR/privacy.html) do Bardo está neste guia e no site dele, e o Bardo pede que você a aceite antes de usar o YouTube; [Configurações › Sobre](bardo:go/settings/about) traz links para ela, para os Termos do YouTube e para a Política de Privacidade do Google, e apaga os dados do YouTube quando você pede; e o Bardo apaga sozinho o que não pode mais guardar ([Onde fica guardado, e por quanto tempo](https://fernandolisboa.github.io/bardo/pt-BR/privacy.html#kept)).

<a id="youtube-form"></a>
## YouTube: o formulário passo a passo

1. No [console do Google Cloud](https://console.cloud.google.com/home/dashboard), escolha o projeto do seu cliente OAuth e copie o **Número do projeto** do cartão **Informações do projeto**.
2. Abra o [formulário de auditoria e extensão de cota](https://support.google.com/youtube/contact/yt_api_form) com a conta do Google dona do projeto. Escolha uma extensão de cota (ou uma nova auditoria, se uma já passou antes).
3. Preencha quem você é e o seu negócio, como o formulário pede.
4. No cliente de API, dê o mesmo nome da tela de consentimento (o formulário pergunta se o nome contém "YouTube"), informe o endereço da política de privacidade, `https://fernandolisboa.github.io/bardo/en-US/privacy.html` (**Política de privacidade online**, em Configurações › Sobre, abre a página), e diga que é um app de desktop instalado no seu próprio computador, não um site.
5. No projeto, cole o número do projeto, escolha **Video Uploading & Account Management** e marque os métodos abaixo. Na descrição de como o cliente usa a API, selecione **Copiar para o formulário** abaixo destes passos e cole o texto. Ele está em inglês, que é o que os revisores do Google leem:

   ```text
   Bardo is a Windows desktop app that one creator installs on their own computer to publish videos to their own YouTube channel. It signs in with the creator's own OAuth client (Desktop app, loopback redirect, PKCE). Every upload is reviewed and confirmed by the creator first; nothing is posted on a timer the creator did not set.

   YouTube Data API v3, with the creator's OAuth token:
   - videos.insert (resumable upload): uploads a rendered video with its title, description, tags, privacy status, publish time, made-for-kids answer and altered or synthetic content disclosure.
   - videos.list (part=snippet,status): reads back the processing and privacy status and the publish time of the creator's own uploads.
   - videos.update (part=status): changes or cancels the publish time of the creator's scheduled videos.
   - channels.list (mine=true): shows which channel is connected, and confirms once a day that the authorization still works.
   - videos.list (part=statistics): public views, likes and comments of the creator's own posts.

   YouTube Analytics API, with the creator's OAuth token:
   - reports.query (ids=channel==MINE): views, engaged views, watch time, average view, audience retention and, for channels in the YouTube Partner Program, estimated revenue and CPM of the creator's own videos.

   YouTube Data API v3, with the creator's API key (public data only):
   - search.list, videos.list and channels.list: niche research, reading recent public videos, their view counts and their channels' subscriber counts in a market.
   - videos.list (part=statistics): public views, likes and comments of the creator's own posts while their channel is not connected.

   OAuth scopes:
   - https://www.googleapis.com/auth/youtube.upload: upload the creator's videos.
   - https://www.googleapis.com/auth/youtube: read the connected channel, and read and change the status and publish time of the creator's videos.
   - https://www.googleapis.com/auth/yt-analytics.readonly: the owner's analytics reports.
   - https://www.googleapis.com/auth/yt-analytics-monetary.readonly: the revenue and CPM reports.

   Data handling: everything stays on the creator's computer. Tokens and keys are kept in Windows Credential Manager; channel ids, video ids and metrics are kept in a local database. Bardo runs no server. The creator accepts Bardo's privacy policy (https://fernandolisboa.github.io/bardo/en-US/privacy.html) before any YouTube feature works; the app links to it, to the YouTube Terms of Service and to the Google Privacy Policy.

   Retention: nothing read from YouTube is kept more than 30 days, except the connected channel's own numbers while its authorization is confirmed, once a day, within the last 30 days. Niche research, theme rankings and numbers read with the API key are deleted 30 days after reading; a connection unconfirmed for 30 days is removed with its data; a daily job does the deleting. Disconnecting in the app revokes the token at Google and deletes the channel's Authorized Data at once; Settings › About deletes all stored YouTube data on request.

   Sharing: only the creator's AI providers (Anthropic Claude for ideas, TypeSafe JEV for ranking) receive a few aggregate numbers (a niche's competition and trend scores, median views per day, upload volume, and first-week views of the creator's own videos), never videos, titles or channels read from YouTube.
   ```

6. Informe a cota que quer para `videos.insert` e `search.list` e diga por quê, por exemplo quantos vídeos por dia somando seus canais.
7. Anexe as capturas de tela que o formulário pede: a política de privacidade, a página com o link para ela (Configurações › Sobre, a seção **Privacidade e YouTube**), a tela de consentimento do Google ao conectar um canal e a revisão de envio do Bardo.
8. Na conta de demonstração, dê ao revisor uma conta do Google de teste com um canal de teste, nunca a sua.

A equipe do Google responde por e-mail; a página não dá um prazo.

<a id="youtube-after"></a>
## YouTube: o que muda depois da aprovação

- Seu projeto recebe a cota que os revisores concederam, na página **Cotas** do console do Google Cloud. O Bardo não faz contagem própria da cota do YouTube: ele descobre o limite pelas respostas do YouTube, então usa o novo na hora.
- Sob a regra de 2020, os envios a partir daí ficam públicos ou não listados como você escolheu. Os vídeos já mantidos privados continuam privados: deixe-os públicos no YouTube Studio.
- Nada muda no Bardo nem nas configurações dele. O Google pode auditar o projeto de novo mais tarde; mantenha a política de privacidade e as regras das políticas em dia.

<a id="tiktok-need"></a>
## TikTok: você precisa da auditoria?

Não, e ela nem passaria. As [diretrizes de compartilhamento de conteúdo](https://developers.tiktok.com/doc/content-sharing-guidelines) do TikTok listam "uma ferramenta para ajudar a enviar conteúdo para as contas que você ou sua equipe administram" como inaceitável, e as [diretrizes da Revisão do App](https://developers.tiktok.com/doc/app-review-guidelines) dizem que apps não podem ser para uso privado ou pessoal. Uma configuração pessoal do Bardo é exatamente isso.

E ela também não é necessária. O Bardo envia os vídeos do TikTok para a sua caixa de entrada como rascunhos, pelo sandbox do app, com até 10 contas suas como usuários-alvo. A auditoria rege o Direct Post (publicar sem o app do TikTok), que o Bardo não implementa: o TikTok restringe clientes de Direct Post não auditados a posts privados de contas privadas, 5 usuários por dia.

<a id="tiktok-asks"></a>
## TikTok: o que a Revisão do App pede

Para um app destinado ao público, as [diretrizes da Revisão do App](https://developers.tiktok.com/doc/app-review-guidelines) do TikTok pedem:

- um site que seja mais do que uma página de entrada ou de login, com links visíveis para a política de privacidade e os termos de serviço;
- um ícone e uma descrição do app;
- pelo menos um vídeo de demonstração (até cinco) do fluxo completo, gravado no sandbox;
- uma explicação detalhada de como cada produto e escopo é usado;
- para o Direct Post, a tela de publicação que as [diretrizes de compartilhamento de conteúdo](https://developers.tiktok.com/doc/content-sharing-guidelines) exigem: o apelido do criador, uma visibilidade que o criador escolhe sem valor padrão, comentários, duetos e costuras desligados até o criador ligá-los, a declaração de conteúdo comercial, e o consentimento expresso do criador antes de qualquer envio.

Apps ainda em desenvolvimento ou em teste não são aprovados.

<a id="tiktok-after"></a>
## TikTok: o que muda depois da aprovação

Para a sua configuração, nada: o sandbox continua funcionando e os rascunhos continuam chegando na sua caixa de entrada. Um app aprovado só faria diferença se o Bardo tivesse um app próprio para outras pessoas, com o Direct Post feito pelas regras do TikTok; essa é uma decisão à parte, não faz parte do Bardo hoje.

<a id="instagram-need"></a>
## Instagram: você precisa da Revisão do App?

Não. A [visão geral da Plataforma do Instagram](https://developers.facebook.com/docs/instagram-platform/overview/) da Meta diz que, se o app só atende à sua conta profissional do Instagram ou a uma conta que você administra, o Acesso Padrão é tudo de que ele precisa. O Acesso Padrão não exige Revisão do App nem Verificação da Empresa: ele atende a todo usuário do Facebook com uma função no app (e num portfólio empresarial que o reivindicou), que é você.

Mantenha o app no modo **Desenvolvimento**. A página de [modos do app](https://developers.facebook.com/docs/development/build-and-test/app-modes/) da Meta diz que um app no modo **Ao vivo** só pode usar as permissões aprovadas pela Revisão do App, mesmo para quem tem uma função nele, então passá-lo para Ao vivo sem uma revisão impede o Bardo de publicar. A mesma página diz que os dados que um app cria no modo Desenvolvimento, "como posts de teste", só podem ser vistos por quem tem uma função nele. Ainda não está confirmado se isso vale para Reels publicados na sua própria conta: confira o seu primeiro Reel num navegador em que você não esteja conectado.

<a id="instagram-asks"></a>
## Instagram: o que a Revisão do App pede

O Acesso Avançado, para contas que você não tem nem administra, exige a [Revisão do App](https://developers.facebook.com/docs/app-review/) e a [Verificação da Empresa](https://developers.facebook.com/docs/development/release/business-verification/). O [guia de envio](https://developers.facebook.com/docs/resp-plat-initiatives/individual-processes/app-review/submission-guide/) pede:

- um ícone do app (1024 × 1024), uma URL de política de privacidade e uma URL ou instruções de exclusão de dados do usuário;
- para cada permissão (`instagram_basic`, `instagram_content_publish`, `instagram_manage_insights`, `pages_show_list`, `pages_read_engagement`): uma descrição própria de como o app a usa, uma gravação de tela mostrando esse uso e pelo menos uma chamada bem-sucedida nos últimos 30 dias;
- instruções para o revisor acessar o app, sem as suas credenciais pessoais;
- as perguntas sobre tratamento de dados, e a Verificação da Empresa por trás do app.

A Meta diz que a decisão costuma sair em até uma semana. Apps com Acesso Avançado também fazem a Verificação Anual de Uso de Dados da Meta.

<a id="instagram-after"></a>
## Instagram: o que muda depois da aprovação

Para as suas próprias contas, nada: o Acesso Padrão já as atende. O Acesso Avançado deixaria contas do Instagram cujos donos não têm função no seu app se conectarem a ele, o que o Bardo não oferece: cada pessoa que usa o Bardo registra o próprio app.

<a id="x-need"></a>
## X: você precisa de revisão?

Não. A página [Getting access](https://docs.x.com/x-api/getting-started/getting-access) do X configura um app em três passos (cadastro no Developer Console, criação do app, guarda das credenciais), sem revisão no meio. O acesso é [pago por uso](https://docs.x.com/x-api/getting-started/pricing) com créditos que você compra, então o que limita um app são os créditos, o limite de gastos e os limites de requisição do X (para posts, 100 a cada 15 minutos por conta e 10.000 por dia por app), não uma aprovação.

<a id="x-asks"></a>
## X: o que o X pede

- No Developer Console: aceitar o Developer Agreement and Policy e informações básicas sobre como você vai usar a API.
- Para o app: um nome, uma descrição e um caso de uso.
- Depois, seguir a Developer Policy e as regras de automação do X: o Bardo posta só o que você revisou, da sua própria conta, um post por vídeo.

<a id="x-after"></a>
## X: o que muda depois da aprovação

Não há aprovação a esperar. Volume acima do que o pago por uso permite (3 milhões de leituras de posts por mês, longe de qualquer uso do Bardo) exige o plano Enterprise do X, de que o Bardo não precisa.
