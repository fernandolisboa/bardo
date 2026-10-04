---
id: connect-tiktok
title: Conectar o TikTok
group: publishing
---

# Conectar o TikTok

O Bardo entra no TikTok com um app que você registra na sua própria conta do TikTok for Developers. O Bardo não traz app próprio, então o app, o sandbox dele e qualquer auditoria são seus. Este guia configura esse app uma vez; depois disso, cada conta conecta pelo cartão da conta de rede em poucos cliques.

O Bardo manda os vídeos do TikTok para a sua caixa de entrada como **rascunhos**: você finaliza a legenda, escolhe quem pode assistir e posta (ou agenda) no app do TikTok. O Bardo nunca posta no TikTok sozinho.

<a id="need"></a>
## Do que você precisa

- A conta do TikTok de onde você posta.
- Uma conta no [TikTok for Developers](https://developers.tiktok.com/) (cadastre-se com qualquer e-mail; não precisa ser a conta que posta).

O site do TikTok for Developers é em inglês, então os nomes de menus e campos abaixo ficam em inglês.

<a id="register"></a>
## 1. Registre o app

1. No TikTok for Developers, abra o menu do seu perfil › **Manage apps** e escolha **Connect an app**. Registre o app na sua conta individual (ou numa organização, se você tiver uma).
2. Preencha as **Basic information**: um **App icon** (1024 × 1024 px), um **App name** (por exemplo `Bardo`), uma **Category** e uma **Description**; a descrição aparece na página de autorização do TikTok. O TikTok também pede uma **Terms of Service URL**, uma **Privacy Policy URL** e, para **Desktop**, um site; quaisquer páginas que você controle servem enquanto o app fica no sandbox.
3. Em **Platforms**, escolha **Desktop**.

A seção **Credentials** mostra o **Client key** e o **Client secret** do app. Você vai copiá-los do sandbox no passo 3, não daqui.

<a id="products"></a>
## 2. Adicione os produtos e os escopos

1. Escolha **Add products** e adicione:
   - **Login Kit**;
   - **Content Posting API**. Deixe o **Direct Post** desligado: o Bardo só envia rascunhos.
2. No **Login Kit**, abra as configurações de **Desktop** e adicione exatamente este **Redirect URI**, com a barra final: `http://127.0.0.1:*/callback/`. O `*` é a porta curinga do TikTok: enquanto você autoriza, o Bardo escuta uma vez em `127.0.0.1`, numa porta aleatória, em `/callback/`, e o TikTok manda o navegador de volta para lá.
3. Em **Scopes**, confira se o app tem os três (o Bardo pede todos de uma vez):
   - `user.info.basic`: o nome da conta, mostrado no cartão;
   - `video.upload`: mandar um vídeo para a sua caixa de entrada como rascunho;
   - `video.list`: ler os números dos seus vídeos para as métricas do dono.

<a id="sandbox"></a>
## 3. Crie um sandbox e adicione a sua conta

O Bardo funciona a partir do sandbox do app; ele nunca precisa do app aprovado.

1. Ao lado do nome do app, mude para **Sandbox** e escolha **Create Sandbox**. Dê um nome (por exemplo `Bardo`) e clone a configuração da produção, para o Login Kit, a Content Posting API, o redirect URI e os escopos virem junto. Confira e escolha **Apply changes**.
2. Em **Sandbox settings › Target users**, escolha **Add account**, entre com a conta do TikTok de onde você posta e aceite os termos de desenvolvedor. O TikTok pode levar até uma hora para mostrá-la. Só usuários de teste (*target users*) podem autorizar um app em sandbox (até 10 por sandbox).
3. Copie o **Client key** (começa com `sb`) e o **Client secret** do sandbox, nas **Credentials** dele. Um sandbox tem credenciais próprias; as da produção não fazem login de um usuário de teste.

<a id="review"></a>
## Por que o app não vai para revisão

As diretrizes de Compartilhamento de Conteúdo e de Revisão de Apps do TikTok rejeitam apps de uso privado ou pessoal; "uma ferramenta para ajudar a enviar conteúdo às contas que você ou sua equipe gerenciam" aparece como não aceitável. Uma configuração pessoal do Bardo é exatamente isso, então não passaria, e nem precisa: enviar rascunhos à sua própria caixa de entrada não exige auditoria. O que uma auditoria acrescentaria é o Direct Post (postar sem o app do TikTok), que o Bardo não implementa.

**Mantenha a conta privada enquanto testa.** O TikTok documenta que toda conta que posta por um app sem auditoria precisa estar privada no momento do post, e que esses posts ficam visíveis só para quem criou (`SELF_ONLY`). O TikTok escreve essas regras para o Direct Post; se ele as aplica também a rascunhos mandados de um sandbox é conferido no teste de regressão da publicação. Até lá, deixe a conta como **Conta privada** em **Configurações e privacidade › Privacidade** no TikTok.

<a id="save"></a>
## 4. Salve o app no Bardo

1. Abra [Configurações › Redes](bardo:go/settings/networks).
2. Cole o client key e o client secret em **TikTok · app do TikTok for Developers** e escolha **Salvar**.

Os dois ficam no Gerenciador de Credenciais do Windows, na sua conta do Windows, por perfil do Bardo. Eles nunca chegam ao banco de dados, aos logs nem às mensagens de erro do Bardo, e o secret não aparece mais na tela; o cartão mostra os quatro últimos caracteres dele.

<a id="connect"></a>
## 5. Conecte a conta

1. Abra [Contas](bardo:go/accounts), escolha o canal e adicione (ou abra) a conta do TikTok dele.
2. Escolha **Conectar**. O navegador abre a página de autorização do TikTok.
3. Entre com a conta do usuário de teste e permita todas as permissões. Deixar uma desligada faz o Bardo recusar a conexão e revogar o que foi dado.
4. O navegador mostra "O Bardo está conectado" e o cartão mostra **Conectada como (nome de exibição)**.

O Bardo espera cinco minutos pelo navegador; **Cancelar** para de esperar.

Os tokens de acesso e de renovação ficam no Gerenciador de Credenciais do Windows, por perfil e por conta de rede. O banco de dados do Bardo guarda só o `open_id` e o nome de exibição da conta, os escopos, a validade do token e a última renovação.

<a id="day-to-day"></a>
## No dia a dia

- O token de acesso do TikTok dura 24 horas e o de renovação, 365 dias. O Bardo renova o acesso quando ele é usado ou verificado, e quando o app abre se ele tiver expirado, então a conexão dura enquanto o Bardo for aberto de vez em quando. O TikTok pode devolver um token de renovação novo a cada renovação; o Bardo sempre guarda o mais novo.
- **Verificar** renova o acesso se ele estiver perto de expirar e lê de novo o nome de exibição.
- **Reconexão necessária** quer dizer que o TikTok recusou renovar o acesso: você removeu o Bardo dos apps autorizados da conta, ou o token de renovação ficou um ano sem uso. Escolha **Reconectar**. Um client key ou client secret que o TikTok não aceita mais (por exemplo, depois de você gerar um secret novo) mostra "O TikTok não reconhece o client key ou o client secret": salve o novo em **Configurações › Redes**.
- **Desconectar** revoga o acesso do Bardo no TikTok e esquece os tokens. Revogar exige as credenciais do app: se elas foram removidas de **Configurações › Redes**, ou não for possível falar com o TikTok, o Bardo esquece os tokens mesmo assim e pede que você remova o acesso dele, nas permissões de apps e serviços das configurações de segurança do app do TikTok.
- Uma conta conectada precisa ser desconectada antes de ser removida.

<a id="draft"></a>
## Mandar um rascunho

1. Na etapa **Publicação** de um projeto, escolha a conta do TikTok e **Revisar envio**. A revisão mostra o arquivo renderizado, a conta conectada e a legenda para colar. Um arquivo que o TikTok não aceitaria aparece no lugar, com o que mudar.
2. Marque **Lembrar do rótulo de conteúdo gerado por IA** quando o vídeo tiver conteúdo realista de IA (vem marcado quando a narração usa uma voz realista). O TikTok define o rótulo no app dele; o Bardo lembra você quando o rascunho chegar.
3. Escolha **Enviar à caixa de entrada do TikTok**. O Bardo manda o arquivo em partes e retoma da última que o TikTok confirmou se a conexão cair ou você parar. Depois de uma hora o TikTok esquece um envio não terminado, então retomar mais tarde manda o arquivo de novo.
4. Quando o TikTok tem o rascunho, o post mostra **Rascunho no TikTok** com a legenda e um botão **Copiar legenda**. Abra a caixa de entrada do TikTok no app, cole a legenda, ative o rótulo de IA se for lembrado e poste (ou agende).
5. De volta ao Bardo, escolha **Marcar como publicado** e cole o link do post, para o Bardo acompanhá-lo como qualquer outro post.

<a id="metrics"></a>
## Números na tela Desempenho

Com a conta conectada, toda sincronização de métricas lê as visualizações, curtidas, comentários e compartilhamentos dos posts do TikTok vinculados ao canal, 20 por requisição, sem chave do YouTube. O TikTok não informa tempo de exibição, retenção nem receita, então esses campos ficam vazios.

- O TikTok devolve só os posts públicos da conta. Um post que ele deixa de fora (apagado, tornado privado ou de outra conta) aparece como **Não encontrado** e mantém os números que tinha.
- Um rascunho é acompanhado depois que você o marca como publicado com o link.
- Sem conta conectada, um post vinculado guarda só o link. Quando a conta precisa reconectar, as sincronizações pulam os posts dela até a reconexão.

<a id="limits"></a>
## Limites para conhecer

- O TikTok mantém no máximo 5 rascunhos de um app esperando na sua caixa de entrada em 24 horas. O Bardo conta os rascunhos que mandou e recusa um sexto na revisão, dizendo quando o próximo pode sair. Se o TikTok ainda recusar um (por exemplo, por rascunhos de outro app), o envio espera na fila e o Bardo tenta de novo mais tarde.
- O TikTok aceita MP4, WebM ou MOV em H.264, H.265, VP8 ou VP9, de 23 a 60 quadros por segundo, de 360 a 4096 pixels por lado, até 10 minutos e 4 GB. Os presets do Bardo atendem a isso.
- A Content Posting API não tem campo de agendamento: você agenda o rascunho no app do TikTok quando posta.
