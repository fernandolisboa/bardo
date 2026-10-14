---
id: connect-x
title: Conectar o X
group: publishing
---

# Conectar o X

O Bardo entra no X com um app que você registra na sua própria conta do X Developer Console. O Bardo não tem app próprio, então o app, os créditos e o limite de gastos são seus. Este guia configura o app uma vez; depois disso, cada conta do X se conecta pelo cartão da conta de rede.

O Bardo envia o vídeo para o X e faz o post ele mesmo, com o texto que você revisou, na hora ou no horário que você agendar. A API do X é paga por uso: o X cobra cada post de créditos comprados antes, e o Bardo mostra o preço antes de você confirmar.

<a id="need"></a>
## O que você precisa

- A conta do X em que você posta.
- Uma conta no [X Developer Console](https://console.x.com), entrando com qualquer conta do X (não precisa ser a que posta) e com o Developer Agreement and Policy aceito.
- Um cartão de pagamento para os créditos do app. O X não tem plano gratuito para postar.

<a id="app"></a>
## 1. Criar o app

1. No [Developer Console](https://console.x.com), crie um app novo. Dê um nome (por exemplo `Bardo`), uma descrição e um caso de uso.
2. Abra **User authentication settings** do app e configure:
   - **App permissions**: **Read and write**. O Bardo posta por você; só com leitura, o X recusa o post.
   - **Type of App**: **Web App, Automated App or Bot**. Esse tipo é um cliente confidencial, então o X dá a ele um client secret, que o Bardo usa.
   - **Callback URI / Redirect URL**: exatamente `http://127.0.0.1:47147/callback`. O X confere o endereço caractere por caractere, com a porta, e pede `127.0.0.1` em vez de `localhost`. Enquanto você autoriza, o Bardo escuta uma vez nessa porta do seu próprio computador e o X manda o navegador de volta para lá.
   - **Website URL**: qualquer página sua.
3. Salve, abra **Keys and tokens** e copie o **OAuth 2.0 Client ID** e o **Client Secret**. O X mostra o secret uma vez só; se perder, gere outro e substitua no Bardo.

<a id="credits"></a>
## 2. Comprar créditos e definir um limite

1. No Developer Console, abra a cobrança do app e compre créditos. O X desconta cada chamada deles na hora.
2. Defina um **limite de gastos** (spending limit), para que um erro nunca custe mais do que você escolheu.

Quanto custam as chamadas do Bardo, pelos preços do X em 14/10/2026:

| O quê | Preço do X |
| --- | --- |
| Um post | US$ 0,015 |
| Um post com link no texto | US$ 0,20 |
| Enviar o vídeo | Não é cobrado |
| Ler a conta ao conectar ou ao clicar em **Verificar** | US$ 0,010 |

O X pode mudar os preços; o Developer Console sempre mostra os atuais. Quando os créditos acabam ou o limite é atingido, o X recusa o post e o Bardo avisa: adicione créditos e clique em **Tentar de novo**.

<a id="save"></a>
## 3. Salvar o app no Bardo

1. Abra [Configurações › Redes](bardo:go/settings/networks).
2. Cole o client ID e o secret em **X · app do X Developer Console** e clique em **Salvar**.

Os dois ficam no Gerenciador de Credenciais do Windows, na sua conta do Windows, por perfil do Bardo. Nunca vão para o banco de dados, os logs ou as mensagens de erro do Bardo, e o secret não aparece de novo na tela; o cartão mostra os últimos quatro caracteres.

<a id="connect"></a>
## 4. Conectar a conta

1. Abra [Contas](bardo:go/accounts), escolha o canal e adicione (ou abra) a conta do X dele.
2. Clique em **Conectar**. O navegador abre a página de autorização do X; entre com a conta em que você posta e clique em **Authorize app**.
3. O X manda o navegador de volta ao Bardo, que lê a conta e mostra **Conectada como @usuario**.

O Bardo pede ao X estas permissões de uma vez: `tweet.read` e `users.read` (quem é a conta), `tweet.write` (fazer o post), `media.write` (enviar o vídeo) e `offline.access` (continuar conectado). Se faltar alguma, nada é guardado e o cartão diz qual.

Se o cartão disser que a porta está em uso, outro programa no seu computador ocupa a porta 47147: feche-o, ou espere um minuto se acabou de tentar, e conecte de novo.

<a id="day-to-day"></a>
## No dia a dia

- **Tokens.** O token de acesso do X dura duas horas. O Bardo renova com o refresh token quando vence, e o X entrega um refresh token novo a cada vez, que o Bardo guarda no Gerenciador de Credenciais do Windows.
- **Verificar** renova o token se for a hora e lê de novo o nome de usuário da conta. O X cobra essa leitura (veja [Comprar créditos](#credits)).
- **Reconexão necessária** quer dizer que o X recusou os tokens: você revogou o app nas configurações do X (**Configurações e privacidade › Segurança e acesso à conta › Apps e sessões**), mudou a senha, ou o client secret do app mudou. Clique em **Reconectar**.
- **Desconectar** revoga os tokens do Bardo no X e os esquece. Se o X não responder, o Bardo esquece os tokens mesmo assim; remova o app nas configurações da sua conta do X para terminar.

<a id="upload"></a>
## Postar um vídeo

Na etapa Publicar, uma conta do X conectada oferece **Revisar envio** quando o preset dela está renderizado e o texto escrito.

- **A revisão** mostra o arquivo, a conta e o texto do post como o X recebe (com o rodapé da conta e as hashtags), o preço que o X cobra por ele e **Rótulo de feito com IA**, ligado quando a narração usou uma voz realista. O X não tem título, visibilidade nem capa num post.
- **Enviar e postar** manda o arquivo em partes e, quando o X termina de processar, faz o post. O Bardo confere a cada minuto; se o X ainda estiver processando depois de uns quinze minutos, o post mostra **Ainda processando** e **Verificar de novo** retoma depois.
- **Orçamento.** Com um orçamento para o X na tela [Custos](costs.md), um post que chegaria a ele pergunta antes; **Gerar mesmo assim** posta.
- **Parar e retomar.** Um envio parado ou interrompido continua depois da última parte que o X recebeu. O X guarda um envio por 24 horas; um envio que passou disso, ou que o X descartou, vai de novo do início, no máximo duas vezes.
- **Duração do vídeo.** O X aceita vídeos de até 2 minutos e 20 segundos da maioria das contas, que é a duração do preset do X no Bardo. Vídeos mais longos precisam de uma conta do X que possa postá-los; senão o X recusa o post e o Bardo mostra o motivo dado pelo X.

<a id="schedule"></a>
## Agendar um post

O X não recebe horário de publicação de apps, então na revisão **Quando** oferece **Assim que processar** ou **Agendar**, como num Reel. Com **Agendar**, o Bardo envia o arquivo até 23 horas antes e faz o post ele mesmo na data e hora que você digitar.

Deixe o Bardo aberto nessa hora e o computador ligado, ou ligue o [agente em segundo plano](background-agent.md), que posta com o Bardo fechado enquanto você está conectado no Windows. Se nenhum dos dois estiver rodando, nada é postado: na próxima vez que o Bardo abrir, o post aparece entre os [posts perdidos](missed-posts.md).

<a id="costs"></a>
## O post na tela Custos

Cada post que o Bardo faz é registrado na tela [Custos](costs.md) em X, com o vídeo e o canal a que se refere, pelo preço da tabela de preços (US$ 0,015, ou US$ 0,20 com link). O Bardo cobra como post com link quando o texto tem um endereço ou um domínio que o X transformaria em link, como `exemplo.com`. Um post que o X recusou não é cobrado nem registrado. As leituras da conta ao conectar ou verificar não são registradas; o Developer Console mostra essas.
